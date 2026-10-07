"""Start native vLLM Metal and the Compose app as one development session."""

import argparse
from collections.abc import Callable
import io
import json
import os
from pathlib import Path
import platform
import re
import select
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import time
import traceback
from typing import BinaryIO, TextIO
import urllib.error
import urllib.request


def port_number(value: str) -> int:
    try:
        port = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("port must be an integer") from error
    if not 1 <= port <= 65535:
        raise argparse.ArgumentTypeError("port must be between 1 and 65535")
    return port


def boolean(value: str) -> bool:
    if value not in ("true", "false"):
        raise argparse.ArgumentTypeError("use true or false")
    return value == "true"


def wait_until_ready(server: subprocess.Popen, port: int) -> None:
    # Model loading may take minutes. The process and Ctrl-C, rather than an
    # arbitrary model-size-dependent deadline, bound startup.
    while server.poll() is None:
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=1) as response:
                if response.status == 200:
                    return
        except (urllib.error.URLError, TimeoutError):
            pass
        time.sleep(0.25)
    raise RuntimeError(f"vLLM exited during startup (status {server.returncode})")


def handle_termination(*_) -> None:
    raise KeyboardInterrupt


def parse_arguments(argv: list[str]) -> tuple[argparse.ArgumentParser, argparse.Namespace]:
    """Own deployment commands; native options require an explicit boundary."""
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    commands = parser.add_subparsers(dest="command", required=True)
    up = commands.add_parser(
        "up", help="prepare and start native inference and Compose", allow_abbrev=False,
        epilog="Pass native vLLM options after --, for example: up -- --max-num-seqs 2",
    )
    up.add_argument("--dev", action="store_true", default=True, help="use the development deployment (default)")
    up.add_argument("-d", "--detached", type=boolean, nargs="?", const=True, default=True, metavar="true|false", help="return once ready, leaving services in the background (default: true)")
    up.add_argument("--model", help="override INFERENCE_MODEL from the root .env")
    up.add_argument("--port", type=port_number, help="override INFERENCE_PORT from the root .env")
    down = commands.add_parser("down", help="stop native inference and remove Compose containers, retaining volumes", allow_abbrev=False)
    down.add_argument("--dev", action="store_true", help=argparse.SUPPRESS)
    options = list(argv)
    # Preserve the existing optional --dev prefix and implicit startup command.
    while options[:1] == ["--dev"]:
        options.pop(0)
    if not options or (options[0].startswith("-") and options[0] not in ("-h", "--help")):
        options.insert(0, "up")
    boundary = options.index("--") if "--" in options else len(options)
    native = options[boundary + 1:]
    arguments = parser.parse_args(options[:boundary])
    if arguments.command != "up" and boundary != len(options):
        parser.error("native vLLM options are only accepted by up, after --")
    arguments.vllm_args = native
    return parser, arguments


def stop_deployment(root: Path) -> int:
    """Stop this checkout's supervisors and native servers, retaining volumes."""
    commands = [str(Path(__file__).resolve()), " ".join(native_command(root) + ["serve"])]
    pattern = "|".join(re.escape(command) for command in commands)
    # Exclude this shutdown invocation and its command wrappers. Only PIDs are
    # inspected here; process arguments and environment values are not logged.
    processes = subprocess.run(["ps", "-axo", "pid=,ppid="], text=True, stdout=subprocess.PIPE, check=True)
    parents = dict(tuple(map(int, line.split())) for line in processes.stdout.splitlines())
    excluded = {os.getpid()}
    parent = parents.get(os.getpid(), 0)
    while parent and parent not in excluded:
        excluded.add(parent)
        parent = parents.get(parent, 0)
    print("Stopping the project deployment and vLLM servers...", flush=True)
    matches = subprocess.run(["pgrep", "-f", pattern], text=True, stdout=subprocess.PIPE)
    if matches.returncode not in (0, 1):
        matches.check_returncode()
    pids = {int(value) for value in matches.stdout.split()} - excluded
    # The supervisor owns its children and Compose cleanup. Signal only the
    # outermost matching processes, then wait before removing their containers.
    for pid in sorted(pids):
        if parents.get(pid) in pids:
            continue
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    while pids:
        remaining = subprocess.run(
            ["ps", "-p", ",".join(map(str, sorted(pids))), "-o", "pid=,stat="],
            text=True, stdout=subprocess.PIPE,
        )
        if remaining.returncode not in (0, 1):
            remaining.check_returncode()
        pids = {
            int(pid) for pid, state in (line.split() for line in remaining.stdout.splitlines())
            if not state.startswith("Z")
        }
        if pids:
            time.sleep(0.25)
    print("Stopping and removing the Compose stack...", flush=True)
    subprocess.run(["docker", "compose", "down"], cwd=root, check=True)
    print("Compose shutdown complete. Persistent volumes were retained.", flush=True)
    return 0


class Startup:
    """Write progress to stdout; acknowledge readiness independently of logs."""

    def __init__(self, connection: TextIO | None = None):
        self.connection = connection

    def progress(self, message: str) -> None:
        print(message, flush=True)

    def ready(self) -> None:
        self.progress("Deployment ready. Use just deploy down to stop and remove containers.")
        if self.connection is not None:
            self.connection.write("ready\n")
            self.connection.flush()


class LogStream:
    """Mirror new bytes from the writer's open log, without reopening its path.

    An independent read offset avoids moving the append writer's position.
    Copy bytes unchanged so partial lines, UTF-8, and carriage returns survive.
    """

    def __init__(self, log: BinaryIO, output: BinaryIO):
        self.log = log
        self.output = output
        self.offset = os.fstat(log.fileno()).st_size

    def copy_pending(self) -> None:
        # Snapshot the end so continuous output cannot starve readiness checks.
        end = os.fstat(self.log.fileno()).st_size
        while self.offset < end:
            chunk = os.pread(self.log.fileno(), min(io.DEFAULT_BUFFER_SIZE, end - self.offset), self.offset)
            if not chunk:
                break
            self.output.write(chunk)
            self.offset += len(chunk)
        self.output.flush()


def follow_session(connection: TextIO, logs: LogStream, *, detached: bool) -> bool:
    """Stream until readiness in detached mode, or command completion otherwise."""
    ready = False
    while True:
        # Regular files have no blocking tail read. Poll new bytes every 100 ms;
        # the separate pipe wakes us immediately on readiness or process exit.
        readable, _, _ = select.select([connection], [], [], 0.1)
        logs.copy_pending()
        if readable:
            event = connection.readline()
            if not event:
                return ready
            if event != "ready\n":
                raise RuntimeError("invalid deployment readiness acknowledgement")
            ready = True
            # Readiness may arrive after the preceding log snapshot. Flush its
            # output before returning to the caller.
            logs.copy_pending()
            if detached:
                return ready


def run_session(log_path: Path, run: Callable[[Startup], int], *, detached: bool) -> int:
    """Own log persistence, terminal streaming, and the command's process lifetime.

    Services always write to the log, so detaching its reader cannot break their
    stdout. Foreground commands keep streaming through exit and cleanup. Detached
    startup returns only after the supervisor acknowledges readiness.
    """
    sys.stdout.flush()
    sys.stderr.flush()
    with log_path.open("a+b", buffering=0) as log:
        logs = LogStream(log, sys.stdout.buffer)
        read_fd, write_fd = os.pipe()
        try:
            pid = os.fork()
        except BaseException:
            os.close(read_fd)
            os.close(write_fd)
            raise
        if pid == 0:
            status = 1
            try:
                os.close(read_fd)
                os.setsid()
                with open(os.devnull) as null:
                    os.dup2(null.fileno(), 0)
                os.dup2(log.fileno(), 1)
                os.dup2(log.fileno(), 2)
                # Launcher prints should be as live as child-process output.
                sys.stdout.reconfigure(line_buffering=True, write_through=True)
                sys.stderr.reconfigure(line_buffering=True, write_through=True)
                with os.fdopen(write_fd, "w") as connection:
                    startup = Startup(connection)
                    status = command_status(lambda: run(startup), startup.progress)
            except BaseException:
                traceback.print_exc()
            finally:
                # Never unwind into the invoking process's control flow.
                os._exit(status)
        os.close(write_fd)
        with os.fdopen(read_fd) as connection:
            try:
                print(f"Deployment process: {pid}. Log: {shlex.quote(str(log_path))}", flush=True)
                ready = follow_session(connection, logs, detached=detached)
                if detached and ready:
                    return 0
            except BaseException as error:
                try:
                    os.kill(pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    if isinstance(error, KeyboardInterrupt):
                        # Keep showing cleanup output while cancellation finishes.
                        follow_session(connection, logs, detached=False)
                finally:
                    os.waitpid(pid, 0)
                if isinstance(error, KeyboardInterrupt):
                    # The supervisor already logged the interruption; preserve
                    # its cancellation status without printing it a second time.
                    logs.copy_pending()
                    return 130
                raise
        _, status = os.waitpid(pid, 0)
        logs.copy_pending()
        code = os.waitstatus_to_exitcode(status)
        if detached and not ready:
            print(f"Deployment did not become ready. See {shlex.quote(str(log_path))}.", flush=True)
            if code == 0:
                return 1
        return code if code >= 0 else 128 - code


def native_command(root: Path) -> list[str]:
    """Use this checkout's interpreter; installed entrypoint shebangs can go stale."""
    return [str(root / "tools/deploy/.venv/bin/python"), "-m", "vllm.entrypoints.cli.main"]


def prepare_native_environment(root: Path, startup: Startup) -> list[str]:
    """Install and build the locked native release before starting services."""
    project = root / "tools/deploy"
    startup.progress("Preparing the native inference environment...")
    subprocess.run(["uv", "sync", "--locked", "--project", str(project)], check=True)
    command = native_command(root)
    startup.progress("Preparing native Metal extension (requires Apple Command Line Tools)...")
    subprocess.run([command[0], "-c", "from vllm_metal.metal.build import build; build()"], check=True)
    return command


def cache_arguments(settings: dict[str, str]) -> list[str]:
    """Own cache policy; vLLM owns binding, assigned ports, and socket lifetime."""
    events = {
        "enable_kv_cache_events": True,
        "publisher": "zmq",
        "endpoint": settings.get("INFERENCE_KV_EVENTS_ENDPOINT") or "tcp://*:0",
        "replay_endpoint": settings.get("INFERENCE_KV_EVENTS_REPLAY_ENDPOINT") or "tcp://*:0",
        "topic": settings.get("INFERENCE_KV_EVENTS_TOPIC") or "kv-events",
    }
    return [
        "--enable-prefix-caching",
        "--prefix-caching-hash-algo", "sha256_cbor",
        "--kv-events-config", json.dumps(events),
    ]


def validate_native_arguments(options: list[str]) -> None:
    """Keep deployment policy out of native overrides, including JSON subkeys."""
    owned = {
        "--host", "--port", "--model", "--served-model-name",
        "--enable-scale-out", "--no-enable-scale-out", "--scheduler-cls",
        "--middleware", "--enable-prefix-caching", "--no-enable-prefix-caching",
        "--prefix-caching-hash-algo", "--kv-events-config", "--config",
    }
    for option in options:
        # vLLM accepts underscores, abbreviated long flags, =values, and dotted
        # JSON keys. A config file would introduce another policy owner.
        flag = option.split("=", 1)[0].split(".", 1)[0].replace("_", "-")
        if flag in ("-c", "--") or (
            flag.startswith("--") and any(name.startswith(flag) for name in owned)
        ):
            raise RuntimeError(
                "model, listener, served name, render API, model discovery, and cache policy "
                "are owned by this command; use INFERENCE_KV_EVENTS_ENDPOINT, "
                "INFERENCE_KV_EVENTS_REPLAY_ENDPOINT, and INFERENCE_KV_EVENTS_TOPIC "
                "for KV event configuration; native config files are not accepted"
            )


def main() -> int:
    parser, arguments = parse_arguments(sys.argv[1:])
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        parser.error("vLLM Metal requires an Apple Silicon Mac")
    launcher = Path(__file__).resolve()
    root = launcher.parents[2]
    signal.signal(signal.SIGTERM, handle_termination)
    # Resolve the destination once. Streaming consumes this writer's open file,
    # and every command uses the same output lifecycle.
    log_path = launcher.with_name("deploy.log")
    if arguments.command == "down":
        return run_session(log_path, lambda _: stop_deployment(root), detached=False)
    return run_session(log_path, lambda startup: run_deployment(arguments, root, startup), detached=arguments.detached)


def run_deployment(arguments: argparse.Namespace, root: Path, startup: Startup) -> int:
    if not root.joinpath(".env").exists():
        startup.progress("Creating .env from .env.example...")
        shutil.copyfile(root / ".env.example", root / ".env")
    # Compose owns .env loading, interpolation, and shell overrides. Capture its
    # configuration without logging it: it can contain service credentials.
    startup.progress("Reading Compose configuration...")
    try:
        configured = subprocess.run(
            ["docker", "compose", "config", "--environment"],
            cwd=root, text=True, capture_output=True, check=True,
        )
    except subprocess.CalledProcessError as error:
        raise RuntimeError(
            f"Could not read Compose configuration (exit status {error.returncode}). "
            "Check Docker availability and .env settings."
        ) from error
    settings = dict(
        line.split("=", 1) for line in configured.stdout.splitlines() if "=" in line
    )
    model = arguments.model if arguments.model is not None else settings.get("INFERENCE_MODEL", "")
    if not model.strip():
        raise RuntimeError("set INFERENCE_MODEL in .env or supply --model")
    try:
        port = arguments.port if arguments.port is not None else port_number(settings.get("INFERENCE_PORT", ""))
        configured_args = shlex.split(settings.get("VLLM_ARGS", ""))
    except (argparse.ArgumentTypeError, ValueError):
        raise RuntimeError("set a valid INFERENCE_PORT and shell-quoted VLLM_ARGS in .env")
    extra = configured_args + arguments.vllm_args
    validate_native_arguments(extra)
    environment = os.environ.copy()
    # The process being launched owns Compose's model and backend connection.
    environment["INFERENCE_MODEL"] = model
    environment["INFERENCE_PORT"] = str(port)
    environment["INFERENCE_ENDPOINT"] = f"http://host.docker.internal:{port}/v1"
    # Both native hooks are installed with the release; they publish resolved
    # model configuration for gateway startup, without shared asset paths.
    environment["PYTHONPATH"] = os.pathsep.join(filter(None, [str(root), environment.get("PYTHONPATH")]))
    # The pinned source install has shader sources, not release metallibs.
    # This selects upstream's MLX shader compilation during worker warm-up;
    # the C++ extension is built below, before the server starts.
    environment["VLLM_METAL_BUILD_FROM_SOURCE"] = "1"
    environment["VLLM_KV_EVENTS_USE_INT_BLOCK_HASHES"] = "0"
    if settings.get("VLLM_HOST_IP"):
        environment["VLLM_HOST_IP"] = settings["VLLM_HOST_IP"]
    native = prepare_native_environment(root, startup)
    # Never mistake another server's health endpoint for this child's startup.
    with socket.socket() as listener:
        # Match vLLM's listener so recently closed connections allow a restart.
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            listener.bind(("0.0.0.0", port))
        except OSError as error:
            raise RuntimeError(
                f"Cannot use native vLLM port {port}: {error.strerror}. "
                "Stop its current server or choose another --port."
            ) from error
    startup.progress(f"Starting native vLLM on port {port}...")
    command = [
        *native, "serve", model,
        "--served-model-name", model,
        "--host", "0.0.0.0", "--port", str(port),
        "--enable-auto-tool-choice", "--tool-call-parser", "hermes",
        # Exposes /v1/responses/render on this same inference server.
        "--enable-scale-out",
        *cache_arguments(settings),
        "--scheduler-cls", "tools.deploy.model_config.ExportingScheduler",
        "--middleware", "tools.deploy.model_config.ModelConfigMiddleware",
        *extra,
    ]
    # Child processes inherit stdout and stderr, keeping their output live.
    server = subprocess.Popen(command, start_new_session=True, env=environment)
    compose_started = False
    try:
        startup.progress("Waiting for native model readiness. Containers start after the model is ready; loading may take several minutes...")
        wait_until_ready(server, port)
        startup.progress("Native vLLM is ready.")
        # Include partial startup failures in cleanup; volumes are preserved.
        compose_started = True
        startup.progress("Building and starting Compose services...")
        subprocess.run(
            ["docker", "compose", "up", "--wait"],
            cwd=root, env=environment, check=True,
        )
        startup.ready()
        status = server.wait()
        print(f"vLLM exited (status {status}). Stopping the deployment...", flush=True)
        return status
    finally:
        try:
            if compose_started:
                print("Stopping Compose services...", flush=True)
                subprocess.run(
                    ["docker", "compose", "stop"],
                    cwd=root, env=environment, check=True,
                )
                print("Compose services stopped. Containers and persistent volumes were retained.", flush=True)
        finally:
            # Include workers even if the server leader has already exited.
            print("Stopping native vLLM and its workers...", flush=True)
            try:
                os.killpg(server.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            if server.poll() is None:
                try:
                    server.wait(timeout=10)
                except subprocess.TimeoutExpired as error:
                    print(f"vLLM did not stop within {error.timeout:g} seconds; forcing shutdown...", flush=True)
                    os.killpg(server.pid, signal.SIGKILL)
                    server.wait()
            print("Native vLLM server stopped.", flush=True)


def command_status(run: Callable[[], int], report: Callable[[str], None] = print) -> int:
    """Preserve command failures for both foreground and detached startup."""
    try:
        return run()
    except KeyboardInterrupt:
        report("Deployment interrupted.")
        return 130
    except subprocess.CalledProcessError as error:
        report(f"Deployment command failed (exit status {error.returncode}); see its output in the deployment log or terminal.")
        return error.returncode if error.returncode > 0 else 128 - error.returncode
    except OSError as error:
        report(f"Deployment failed: {error.strerror}.")
        return 1
    except RuntimeError as error:
        report(f"Deployment failed: {error}")
        return 1


if __name__ == "__main__":
    raise SystemExit(command_status(main))
