"""Verify that one dev session owns the native server and Compose processes."""

import argparse
from contextlib import ExitStack, contextmanager
import io
import os
from pathlib import Path
import select
import signal
import socket
import subprocess
import tempfile
import textwrap
import time
import unittest
from unittest.mock import Mock, call, patch

import serve_mlx


class DeploymentTests(unittest.TestCase):
    def launch(self, arguments=(), *, fail_compose=False, interrupt=False, fail_startup=False, fail_stop=False, build_error=None, output=None):
        server = Mock(pid=12345)
        server.poll.return_value = None if interrupt or fail_compose else 1
        server.wait.side_effect = [KeyboardInterrupt(), 0] if interrupt else None
        server.wait.return_value = 1
        configuration = Mock(stdout="INFERENCE_MODEL=example/model\nINFERENCE_PORT=9000\nVLLM_ARGS=--max-model-len 2048\n")
        failure = subprocess.CalledProcessError(1, ["docker", "compose", "up"])
        stop_failure = subprocess.CalledProcessError(1, ["docker", "compose", "stop"])
        calls = [configuration, failure if fail_compose else Mock(), stop_failure if fail_stop else Mock()]
        with ExitStack() as stack:
            arguments = list(arguments)
            if arguments[:1] == ["up"]:
                arguments.pop(0)
            stack.enter_context(patch.object(serve_mlx.sys, "argv", ["serve_mlx", "up", "--detached=false", *arguments]))
            stack.enter_context(patch.object(serve_mlx.platform, "system", return_value="Darwin"))
            stack.enter_context(patch.object(serve_mlx.platform, "machine", return_value="arm64"))
            stack.enter_context(patch.object(serve_mlx.Path, "exists", return_value=True))
            stack.enter_context(patch.object(serve_mlx.socket, "socket"))
            stack.enter_context(patch.object(serve_mlx.signal, "signal"))
            stack.enter_context(patch.object(serve_mlx, "run_session", side_effect=lambda path, run, **options: run(serve_mlx.Startup())))
            start = stack.enter_context(patch.object(serve_mlx.subprocess, "Popen", return_value=server))

            def build(root, startup):
                start.assert_not_called()
                if build_error is not None:
                    raise build_error
                return root / "tools/deploy/.venv/bin/vllm"

            stack.enter_context(patch.object(serve_mlx, "prepare_native_environment", side_effect=build))
            run = stack.enter_context(patch.object(serve_mlx.subprocess, "run", side_effect=calls))
            stop = stack.enter_context(patch.object(serve_mlx.os, "killpg"))
            stack.enter_context(patch.object(serve_mlx, "wait_until_ready", side_effect=RuntimeError("startup failed") if fail_startup else None))
            stack.enter_context(patch.object(serve_mlx.sys, "stdout", output if output is not None else io.StringIO()))
            if build_error is not None:
                with self.assertRaises(type(build_error)) as raised:
                    serve_mlx.main()
                self.assertIs(raised.exception, build_error)
            elif fail_compose or fail_stop:
                with self.assertRaises(subprocess.CalledProcessError):
                    serve_mlx.main()
            elif interrupt:
                with self.assertRaises(KeyboardInterrupt):
                    serve_mlx.main()
            elif fail_startup:
                with self.assertRaises(RuntimeError):
                    serve_mlx.main()
            else:
                self.assertEqual(serve_mlx.main(), 1)
        return server, start, run, stop

    def test_foreground_passes_compose_configuration_to_both_processes(self):
        server, start, run, stop = self.launch()
        command = start.call_args.args[0]
        self.assertEqual(command[1:3], ["serve", "example/model"])
        self.assertEqual(command[command.index("--served-model-name") + 1], "example/model")
        self.assertEqual(command[command.index("--port") + 1], "9000")
        self.assertEqual(command[-2:], ["--max-model-len", "2048"])
        self.assertNotIn("--detached=false", command)
        self.assertTrue(start.call_args.kwargs["start_new_session"])
        self.assertEqual(start.call_args.kwargs["env"]["VLLM_METAL_BUILD_FROM_SOURCE"], "1")
        self.assertEqual(run.call_args_list[0].args[0], ["docker", "compose", "config", "--environment"])
        self.assertTrue(run.call_args_list[0].kwargs["capture_output"])
        self.assertEqual(run.call_args_list[1].args[0], ["docker", "compose", "up", "--wait"])
        environment = run.call_args_list[1].kwargs["env"]
        self.assertEqual(environment["INFERENCE_MODEL"], "example/model")
        self.assertEqual(environment["INFERENCE_PORT"], "9000")
        self.assertEqual(environment["INFERENCE_ENDPOINT"], "http://host.docker.internal:9000/v1")
        self.assertEqual(run.call_args_list[2].args[0], ["docker", "compose", "stop"])
        self.assertEqual(run.call_args_list[2].kwargs["env"], environment)
        for deployment_step in run.call_args_list[1:]:
            self.assertNotIn("capture_output", deployment_step.kwargs)
            self.assertNotIn("stdout", deployment_step.kwargs)
            self.assertNotIn("stderr", deployment_step.kwargs)
            self.assertTrue(deployment_step.kwargs["check"])
        stop.assert_called_once_with(12345, signal.SIGTERM)

    def test_false_values_run_attached_and_are_not_forwarded_to_vllm(self):
        for flags in (("-d=false",), ("--detached", "false"), ("-d", "false")):
            with self.subTest(flags=flags):
                _, start, _, _ = self.launch(flags)
                self.assertEqual(start.call_args.args[0][1], "serve")
                self.assertTrue(start.call_args.kwargs["start_new_session"])
                self.assertNotIn("-d", start.call_args.args[0])
                self.assertNotIn("--detached", start.call_args.args[0])

    def test_invalid_detached_values_fail_before_launch(self):
        for value in ("invalid", "0", "1"):
            with self.subTest(value=value), patch.object(serve_mlx.sys, "argv", ["serve_mlx", f"--detached={value}"]), patch.object(serve_mlx.sys, "stderr", io.StringIO()), patch.object(serve_mlx.subprocess, "Popen") as launch:
                with self.assertRaises(SystemExit) as error:
                    serve_mlx.main()
                self.assertEqual(error.exception.code, 2)
                launch.assert_not_called()

    def test_unknown_commands_and_unseparated_native_options_start_nothing(self):
        for arguments in (("upp",), ("up", "--max-num-seqs", "2"), ("down", "--model", "example/model"), ("down", "--", "--max-num-seqs", "2")):
            with self.subTest(arguments=arguments), patch.object(serve_mlx.sys, "argv", ["serve_mlx", *arguments]), patch.object(serve_mlx.sys, "stderr", io.StringIO()), patch.object(serve_mlx.subprocess, "Popen") as start, patch.object(serve_mlx.subprocess, "run") as run:
                with self.assertRaises(SystemExit) as error:
                    serve_mlx.main()
                self.assertEqual(error.exception.code, 2)
                start.assert_not_called()
                run.assert_not_called()

    def test_explicit_and_implicit_startup_preserve_native_argument_boundary(self):
        options = ["--model", "example/model", "--port", "9001", "--", "--max-num-seqs", "2"]
        for prefix in ([], ["up"], ["--dev"], ["--dev", "up"], ["up", "--dev"]):
            with self.subTest(prefix=prefix):
                _, arguments = serve_mlx.parse_arguments([*prefix, *options])
                self.assertEqual(arguments.command, "up")
                self.assertEqual(arguments.model, "example/model")
                self.assertEqual(arguments.port, 9001)
                self.assertEqual(arguments.vllm_args, ["--max-num-seqs", "2"])

    def test_shutdown_does_not_prepare_dependencies_or_signal_its_ancestors(self):
        root = Path(serve_mlx.__file__).resolve().parents[2]
        processes = Mock(stdout="100 90\n90 80\n80 1\n1 0\n200 1\n201 200\n")
        matches = Mock(returncode=0, stdout="100\n90\n200\n201\n")
        with ExitStack() as stack:
            stack.enter_context(patch.object(serve_mlx.sys, "argv", ["serve_mlx", "down"]))
            stack.enter_context(patch.object(serve_mlx.platform, "system", return_value="Darwin"))
            stack.enter_context(patch.object(serve_mlx.platform, "machine", return_value="arm64"))
            stack.enter_context(patch.object(serve_mlx.os, "getpid", return_value=100))
            stack.enter_context(patch.object(serve_mlx.sys, "stdout", io.StringIO()))
            stack.enter_context(patch.object(serve_mlx.signal, "signal"))
            stack.enter_context(patch.object(serve_mlx, "run_session", side_effect=lambda path, run, **options: run(serve_mlx.Startup())))
            prepare = stack.enter_context(patch.object(serve_mlx, "prepare_native_environment"))
            running = Mock(returncode=0, stdout="200 S\n201 S\n")
            exited = Mock(returncode=1, stdout="")
            run = stack.enter_context(patch.object(serve_mlx.subprocess, "run", side_effect=[processes, matches, running, exited, Mock()]))
            stack.enter_context(patch.object(serve_mlx.time, "sleep"))
            kill = stack.enter_context(patch.object(serve_mlx.os, "kill"))
            self.assertEqual(serve_mlx.main(), 0)
            prepare.assert_not_called()
            # The supervisor owns its child and must finish before Compose down.
            self.assertEqual(kill.call_args_list, [call(200, signal.SIGTERM)])
            self.assertEqual(run.call_args_list[-2].args[0][0], "ps")
            self.assertEqual(run.call_args_list[-1], call(["docker", "compose", "down"], cwd=root, check=True))

    def test_arguments_override_the_model_and_port_for_both_processes(self):
        _, start, run, _ = self.launch(["--model", "another/model", "--port", "9001", "--", "--max-num-seqs", "2"])
        command = start.call_args.args[0]
        self.assertEqual(command[2], "another/model")
        self.assertEqual(command[command.index("--port") + 1], "9001")
        self.assertEqual(command[-2:], ["--max-num-seqs", "2"])
        environment = run.call_args_list[1].kwargs["env"]
        self.assertEqual(environment["INFERENCE_MODEL"], "another/model")
        self.assertEqual(environment["INFERENCE_ENDPOINT"], "http://host.docker.internal:9001/v1")

    def test_compose_failure_stops_partial_startup_and_native_workers(self):
        server, _, run, stop = self.launch(fail_compose=True)
        self.assertEqual(run.call_args_list[-1].args[0], ["docker", "compose", "stop"])
        stop.assert_called_once_with(12345, signal.SIGTERM)
        server.wait.assert_called_once_with(timeout=10)

    def test_compose_stop_failure_still_stops_native_workers(self):
        _, _, run, stop = self.launch(fail_stop=True)
        self.assertTrue(run.call_args_list[-1].kwargs["check"])
        stop.assert_called_once_with(12345, signal.SIGTERM)

    def test_ctrl_c_stops_compose_and_native_workers(self):
        server, _, run, stop = self.launch(interrupt=True)
        self.assertEqual(run.call_args_list[-1].args[0], ["docker", "compose", "stop"])
        stop.assert_called_once_with(12345, signal.SIGTERM)
        self.assertEqual(server.wait.call_args_list, [call(), call(timeout=10)])

    def test_failed_model_startup_never_starts_or_stops_compose(self):
        _, _, run, stop = self.launch(fail_startup=True)
        self.assertEqual(run.call_count, 1)
        stop.assert_called_once_with(12345, signal.SIGTERM)

    def test_failed_native_build_preserves_error_and_starts_no_services(self):
        for error in (RuntimeError("compiler failed"), KeyboardInterrupt()):
            with self.subTest(error=type(error)):
                _, start, run, stop = self.launch(build_error=error)
                start.assert_not_called()
                self.assertEqual(run.call_count, 1)  # Only reads Compose configuration.
                stop.assert_not_called()

    def test_startup_failure_cannot_accept_another_process_health_response(self):
        server = Mock(returncode=1)
        server.poll.return_value = 1
        with patch.object(serve_mlx.urllib.request, "urlopen") as health:
            with self.assertRaises(RuntimeError):
                serve_mlx.wait_until_ready(server, 9000)
            health.assert_not_called()

    def test_ports_are_validated_before_launch(self):
        for value in ("0", "65536", "-1", "invalid"):
            with self.assertRaises(argparse.ArgumentTypeError):
                serve_mlx.port_number(value)
        self.assertEqual(serve_mlx.port_number("9000"), 9000)


class LogStreamTests(unittest.TestCase):
    def test_follows_the_open_writer_without_replaying_old_output_or_reopening_its_path(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "chosen-output.txt"
            moved = destination.with_suffix(".moved")
            with destination.open("a+b", buffering=0) as log:
                log.write(b"old session\n")
                output = io.BytesIO()
                stream = serve_mlx.LogStream(log, output)
                # A reader tied to a filename would now follow the wrong file.
                destination.rename(moved)
                destination.write_bytes(b"unrelated file\n")
                chunks = (b"partial", b" progress\r", "✓".encode()[:1], "✓".encode()[1:], b"\n")
                expected = b""
                for chunk in chunks:
                    log.write(chunk)
                    writer_offset = log.tell()
                    stream.copy_pending()
                    expected += chunk
                    self.assertEqual(output.getvalue(), expected)
                    self.assertEqual(log.tell(), writer_offset)
                stream.copy_pending()
                self.assertEqual(output.getvalue(), expected)
                self.assertEqual(moved.read_bytes(), b"old session\n" + expected)
                self.assertEqual(destination.read_bytes(), b"unrelated file\n")


# Exercise the real CLI, fork, startup channel, and cleanup in isolated processes.
# Only external services are replaced. The test controls when model loading and
# Compose finish, so a premature successful return is observable at either stage.
SESSION_DRIVER = textwrap.dedent(r"""
    from contextlib import ExitStack
    import os
    from pathlib import Path
    import socket
    import subprocess
    import sys
    from unittest.mock import Mock, patch
    import serve_mlx

    control = socket.socket(fileno=int(sys.argv.pop(1)))
    commands = control.makefile("r")
    root = Path(sys.argv.pop(1))
    serve_mlx.__file__ = str(root / "tools/deploy/serve_mlx.py")

    def load_model(server, port):
        control.sendall(f"{os.getpid()}\n".encode())
        control.sendall(b"loading-model\n")
        os.write(1, b"model stdout is live\n")
        os.write(2, b"model stderr is live\n")
        command = commands.readline().strip()
        if command == "fail":
            raise RuntimeError("model failed to load")
        if command == "exit":
            os._exit(0)

    def compose(command, **options):
        if command[0] == "ps":
            control.sendall(f"{os.getpid()}\n".encode())
            return Mock(stdout=f"{os.getpid()} {os.getppid()}\n{os.getppid()} 1\n")
        if command[0] == "pgrep":
            return Mock(returncode=1, stdout="")
        if command[2] == "down":
            control.sendall(b"stopping-compose\n")
            os.write(1, b"shutdown stdout is live\n")
            os.write(2, b"shutdown stderr is live\n")
            commands.readline()
        if command[2] == "config":
            return Mock(stdout="INFERENCE_MODEL=example/model\nINFERENCE_PORT=9000\n")
        if command[2] == "up":
            control.sendall(b"starting-compose\n")
            os.write(1, b"compose stdout is live\n")
            os.write(2, b"compose stderr is live\n")
            if commands.readline().strip() == "fail":
                raise subprocess.CalledProcessError(7, command)
        elif command[2] == "stop":
            control.sendall(b"compose-stopped\n")
        return Mock(returncode=0)

    def wait_for_server(*, timeout=None):
        if timeout is None:
            while commands.readline().strip() == "emit":
                os.write(1, b"runtime stdout is live\n")
                os.write(2, b"runtime stderr is live\n")
                control.sendall(b"runtime-logged\n")
        return 0

    with ExitStack() as patches:
        server = Mock(pid=12345)
        server.poll.return_value = None
        server.wait.side_effect = wait_for_server
        patches.enter_context(patch.object(serve_mlx, "prepare_native_environment", return_value=Path("vllm")))
        patches.enter_context(patch.object(serve_mlx, "wait_until_ready", side_effect=load_model))
        patches.enter_context(patch.object(serve_mlx.socket, "socket"))
        patches.enter_context(patch.object(serve_mlx.subprocess, "Popen", return_value=server))
        patches.enter_context(patch.object(serve_mlx.subprocess, "run", side_effect=compose))
        patches.enter_context(patch.object(serve_mlx.os, "killpg", side_effect=lambda *args: control.sendall(b"native-stopped\n")))
        raise SystemExit(serve_mlx.command_status(serve_mlx.main))
""")


@unittest.skipUnless(serve_mlx.platform.system() == "Darwin" and serve_mlx.platform.machine() == "arm64", "native deployment requires Apple Silicon")
class SessionTests(unittest.TestCase):
    @contextmanager
    def deployment(self, arguments=(), *, first_event="loading-model\n"):
        """Control external services while exercising the real command lifecycle."""
        with tempfile.TemporaryDirectory() as directory:
            # Keep service control separate from stdin, which detachment closes.
            control, child_control = socket.socketpair()
            control.settimeout(10)
            root = Path(directory)
            launcher = root / "tools/deploy/serve_mlx.py"
            launcher.parent.mkdir(parents=True)
            log_path = launcher.with_name("deploy.log")
            log_path.write_text("Previous session output\n")
            (root / ".env").touch()
            process = subprocess.Popen(
                [serve_mlx.sys.executable, "-c", SESSION_DRIVER, str(child_control.fileno()), str(root), *arguments],
                cwd=Path(serve_mlx.__file__).parent,
                pass_fds=(child_control.fileno(),),
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            )
            child_control.close()
            events = control.makefile("r")
            supervisor = None
            try:
                identity = events.readline()
                if not identity:
                    output, errors = process.communicate(timeout=10)
                    self.fail(f"Deployment fixture exited before its first service operation: {output}\n{errors}")
                supervisor = int(identity)
                self.assertEqual(events.readline(), first_event)
                yield process, control, events, supervisor, log_path
            finally:
                if process.poll() is None:
                    process.terminate()
                try:
                    process.communicate(timeout=10)
                    if supervisor is not None and process.returncode == 0:
                        try:
                            os.kill(supervisor, signal.SIGTERM)
                        except ProcessLookupError:
                            pass
                    # EOF confirms the command's supervisor released its resources.
                    events.read()
                finally:
                    events.close()
                    control.close()
                    process.stdout.close()
                    process.stderr.close()

    def test_detached_startup_waits_for_model_and_compose_then_leaves_services_running(self):
        for flags in ((), ("up",), ("--dev",), ("--dev", "up"), ("up", "--dev"), ("-d",), ("--detached",), ("-d=true",), ("--detached=true",), ("-d", "true"), ("--detached", "true"), ("--detached=false", "-d")):
            with self.subTest(flags=flags), self.deployment(flags) as (process, control, events, supervisor, log_path):
                with self.assertRaises(subprocess.TimeoutExpired):
                    process.wait(timeout=0.1)
                control.sendall(b"continue\n")
                self.assertEqual(events.readline(), "starting-compose\n")
                with self.assertRaises(subprocess.TimeoutExpired):
                    process.wait(timeout=0.1)
                control.sendall(b"continue\n")
                output, errors = process.communicate(timeout=10)
                self.assertEqual(process.returncode, 0, errors)
                self.assertEqual(output.count("Deployment ready."), 1)
                self.assertEqual(output.count("Native vLLM is ready."), 1)
                self.assertNotIn("Previous session output", output)
                for message in ("model stdout is live", "model stderr is live", "compose stdout is live", "compose stderr is live"):
                    self.assertEqual(output.count(message), 1)
                    self.assertEqual(log_path.read_text().count(message), 1)
                os.kill(supervisor, 0)
                control.sendall(b"emit\n")
                self.assertEqual(events.readline(), "runtime-logged\n")
                self.assertIn("runtime stdout is live", log_path.read_text())
                self.assertIn("runtime stderr is live", log_path.read_text())
                control.sendall(b"stop\n")
                self.assertEqual(events.readline(), "compose-stopped\n")
                self.assertEqual(events.readline(), "native-stopped\n")
                self.assertEqual(events.read(), "")

    def read_live_output(self, process, until: bytes) -> bytes:
        """Bound the wait without requiring a newline or process completion."""
        output = bytearray()
        deadline = time.monotonic() + 10
        while until not in output:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([process.stdout], [], [], remaining)[0]:
                self.fail(f"No live output containing {until!r}; received {output!r}")
            chunk = os.read(process.stdout.fileno(), io.DEFAULT_BUFFER_SIZE)
            if not chunk:
                self.fail(f"Command exited before streaming {until!r}; received {output!r}")
            output.extend(chunk)
        return bytes(output)

    def test_logs_stream_while_model_and_compose_are_still_starting(self):
        with self.deployment() as (process, control, events, _, log_path):
            output = self.read_live_output(process, b"model stderr is live")
            self.assertIn(b"model stdout is live", output)
            self.assertIn(b"Waiting for native model readiness.", output)
            self.assertIsNone(process.poll())
            control.sendall(b"continue\n")
            self.assertEqual(events.readline(), "starting-compose\n")
            output += self.read_live_output(process, b"compose stderr is live")
            self.assertIn(b"compose stdout is live", output)
            self.assertIsNone(process.poll())
            control.sendall(b"fail\n")
            final, errors = process.communicate(timeout=10)
            self.assertEqual(process.returncode, 7, errors)
            self.assertIn("Deployment command failed", final)
            self.assertIn("Deployment command failed", log_path.read_text())

    def test_foreground_keeps_streaming_after_readiness_and_saves_shutdown(self):
        for arguments in (("--detached=false",), ("up", "--detached", "false"), ("-d", "false")):
            with self.subTest(arguments=arguments), self.deployment(arguments) as (process, control, events, _, log_path):
                control.sendall(b"continue\n")
                self.assertEqual(events.readline(), "starting-compose\n")
                control.sendall(b"continue\n")
                output = self.read_live_output(process, b"Deployment ready.")
                self.assertIsNone(process.poll())
                control.sendall(b"emit\n")
                self.assertEqual(events.readline(), "runtime-logged\n")
                output += self.read_live_output(process, b"runtime stderr is live")
                self.assertIn(b"runtime stdout is live", output)
                self.assertIsNone(process.poll())
                control.sendall(b"stop\n")
                final, errors = process.communicate(timeout=10)
                self.assertEqual(process.returncode, 0, errors)
                self.assertIn("Native vLLM server stopped.", final)
                self.assertIn("Native vLLM server stopped.", log_path.read_text())
                self.assertIn("runtime stderr is live", log_path.read_text())

    def test_down_streams_and_persists_output_until_cleanup_completes(self):
        with self.deployment(("down",), first_event="stopping-compose\n") as (process, control, events, _, log_path):
            output = self.read_live_output(process, b"shutdown stderr is live")
            self.assertIn(b"shutdown stdout is live", output)
            self.assertIsNone(process.poll())
            control.sendall(b"continue\n")
            final, errors = process.communicate(timeout=10)
            self.assertEqual(process.returncode, 0, errors)
            self.assertIn("Compose shutdown complete.", final)
            saved = log_path.read_text()
            self.assertIn("shutdown stdout is live", saved)
            self.assertIn("shutdown stderr is live", saved)
            self.assertIn("Compose shutdown complete.", saved)

    def test_compose_failure_reaches_caller_after_cleanup(self):
        with self.deployment() as (process, control, events, _, log_path):
            control.sendall(b"continue\n")
            self.assertEqual(events.readline(), "starting-compose\n")
            control.sendall(b"fail\n")
            self.assertEqual(events.readline(), "compose-stopped\n")
            self.assertEqual(events.readline(), "native-stopped\n")
            output, errors = process.communicate(timeout=10)
            self.assertEqual(process.returncode, 7, errors)
            self.assertIn("exit status 7", output)

    def test_model_failure_reaches_caller_without_starting_compose(self):
        with self.deployment() as (process, control, events, _, log_path):
            control.sendall(b"fail\n")
            self.assertEqual(events.readline(), "native-stopped\n")
            output, errors = process.communicate(timeout=10)
            self.assertEqual(process.returncode, 1, errors)
            self.assertIn("model failed to load", output)
            self.assertEqual(events.read(), "")

    def test_interruption_waits_for_startup_cleanup(self):
        for phase in ("model", "compose"):
            for interruption in (signal.SIGINT, signal.SIGTERM):
                with self.subTest(phase=phase, signal=interruption), self.deployment() as (process, control, events, _, log_path):
                    if phase == "compose":
                        control.sendall(b"continue\n")
                        self.assertEqual(events.readline(), "starting-compose\n")
                    process.send_signal(interruption)
                    if phase == "compose":
                        self.assertEqual(events.readline(), "compose-stopped\n")
                    self.assertEqual(events.readline(), "native-stopped\n")
                    output, errors = process.communicate(timeout=10)
                    self.assertEqual(process.returncode, 130, errors)
                    self.assertEqual(output.count("Deployment interrupted."), 1)
                    self.assertIn("Native vLLM server stopped.", output)
                    self.assertEqual(events.read(), "")

    def test_exit_without_readiness_cannot_report_success(self):
        with self.deployment() as (process, control, events, _, log_path):
            control.sendall(b"exit\n")
            output, errors = process.communicate(timeout=10)
            self.assertEqual(process.returncode, 1, errors)
            self.assertIn("Deployment did not become ready.", output)
            self.assertEqual(events.read(), "")


if __name__ == "__main__":
    unittest.main()
