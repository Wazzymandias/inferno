"""Verify that one dev session owns the native server and Compose processes."""

import argparse
from contextlib import ExitStack
import io
from pathlib import Path
import signal
import subprocess
import tempfile
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
            stack.enter_context(patch.object(serve_mlx.sys, "argv", ["serve_mlx", "--detached=false", *arguments]))
            stack.enter_context(patch.object(serve_mlx.platform, "system", return_value="Darwin"))
            stack.enter_context(patch.object(serve_mlx.platform, "machine", return_value="arm64"))
            stack.enter_context(patch.object(serve_mlx.Path, "exists", return_value=True))
            stack.enter_context(patch.object(serve_mlx.socket, "socket"))
            stack.enter_context(patch.object(serve_mlx.signal, "signal"))
            start = stack.enter_context(patch.object(serve_mlx.subprocess, "Popen", return_value=server))

            def build():
                start.assert_not_called()
                if build_error is not None:
                    raise build_error

            stack.enter_context(patch.object(serve_mlx, "build_metal_extension", side_effect=build))
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

    def test_default_and_explicit_detached_flags_preserve_arguments_without_recursive_detachment(self):
        for flags in ((), ("-d",), ("--detached",), ("-d=true",), ("--detached=true",), ("-d", "true"), ("--detached", "true"), ("--detached=false", "-d")):
            with self.subTest(flags=flags):
                self.check_detached_launch(flags)

    def check_detached_launch(self, flags, native_arguments=("--", "--max-num-seqs", "2")):
        with tempfile.TemporaryDirectory() as directory, ExitStack() as stack:
            root = Path(directory)
            script = root / "tools/deploy/serve_mlx.py"
            script.parent.mkdir(parents=True)
            launcher_arguments = [*flags, "--dev", "--model", "example/model", "--port", "9001"]
            arguments = [*launcher_arguments, *native_arguments]
            stack.enter_context(patch.object(serve_mlx, "__file__", str(script)))
            stack.enter_context(patch.object(serve_mlx.sys, "argv", [str(script), *arguments]))
            stack.enter_context(patch.object(serve_mlx.platform, "system", return_value="Darwin"))
            stack.enter_context(patch.object(serve_mlx.platform, "machine", return_value="arm64"))
            output = stack.enter_context(patch.object(serve_mlx.sys, "stdout", io.StringIO()))
            configure = stack.enter_context(patch.object(serve_mlx.subprocess, "run"))

            def start(command, **options):
                self.assertEqual(command, [serve_mlx.sys.executable, str(script.resolve()), *launcher_arguments, "--detached=false", *native_arguments])
                self.assertEqual(options["cwd"], root.resolve())
                self.assertEqual(options["stdin"], subprocess.DEVNULL)
                self.assertEqual(options["stderr"], subprocess.STDOUT)
                self.assertTrue(options["start_new_session"])
                self.assertEqual(Path(options["stdout"].name), root.resolve() / "tools/deploy/deploy.log")
                self.assertFalse(options["stdout"].closed)
                return Mock(pid=4321)

            launch = stack.enter_context(patch.object(serve_mlx.subprocess, "Popen", side_effect=start))
            self.assertEqual(serve_mlx.main(), 0)
            launch.assert_called_once()
            configure.assert_not_called()
            self.assertTrue(launch.call_args.kwargs["stdout"].closed)
            return launch.call_args.args[0][2:]

    def test_detached_without_native_options_still_runs_child_attached(self):
        child_arguments = self.check_detached_launch(("-d",), native_arguments=())
        _, start, _, _ = self.launch(child_arguments)
        self.assertEqual(start.call_args.args[0][1], "serve")

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


if __name__ == "__main__":
    unittest.main()
