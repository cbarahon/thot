#!/usr/bin/env python3
# Purpose: Executes the repository's ordered YAML implementation tasks with Ollama.
"""Sequential YAML task runner for Ollama.

Supported actions:
  create: create a new file (fails if it already exists)
  modify: replace the entire contents of an existing file
  delete: delete an existing file (does not call Ollama)

Task YAML files are moved from todo/ to in-progress/, then to done/ or failure/.
Full task and model logs are written to logs/<task-name>-<timestamp>.log.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import logging
import os
import shutil
import sys
import time
import traceback
from pathlib import Path
from typing import Any

try:
    import requests
    import yaml
except ImportError as exc:
    print(
        f"Missing dependency: {exc.name}. Install with: "
        "python3 -m pip install requests PyYAML",
        file=sys.stderr,
    )
    raise SystemExit(2)

# NAME                    ID              SIZE      MODIFIED    
# qwen3.5.16K:4B          c54d555f81c7    3.3 GB    8 days ago     
# qwen2.5-coder.16K:7B    74cef8e01760    4.7 GB    9 days ago     
# qwen2.5-coder:7B        dae161e27b0e    4.7 GB    9 days ago     
# qwen3.5:4b              d8b0f5e9760c    3.3 GB    11 days ago    
# qwen3:8B                500a1f067a9f    5.2 GB    11 days ago    
# qwen2.5-coder:7b        dae161e27b0e    4.7 GB    12 days ago    
# phi4-mini:latest        78fad5d182a7    2.5 GB    12 days ago  

DEFAULT_MODEL = "qwen2.5-coder.16K:7B"
DEFAULT_OLLAMA_URL = "http://192.168.1.106:11434"
DEFAULT_TIMEOUT = 1200
VALID_ACTIONS = {"create", "modify", "delete"}


def timestamp() -> str:
    return dt.datetime.now().astimezone().strftime("%Y%m%d-%H%M%S")


def configure_task_logger(log_path: Path) -> logging.Logger:
    logger = logging.getLogger(f"task.{log_path.stem}")
    logger.setLevel(logging.DEBUG)
    logger.propagate = False
    logger.handlers.clear()

    handler = logging.FileHandler(log_path, encoding="utf-8")
    handler.setLevel(logging.DEBUG)
    handler.setFormatter(
        logging.Formatter(
            fmt="%(asctime)s %(levelname)s %(message)s",
            datefmt="%Y-%m-%d %H:%M:%S%z",
        )
    )
    logger.addHandler(handler)
    return logger


def required_string(task: dict[str, Any], key: str) -> str:
    value = task.get(key)
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"YAML field {key!r} must be a non-empty string")
    return value


def load_task(task_yaml: Path) -> dict[str, Any]:
    with task_yaml.open("r", encoding="utf-8") as handle:
        data = yaml.safe_load(handle)
    if not isinstance(data, dict):
        raise ValueError("YAML root must be a mapping/object")

    required_string(data, "id")
    required_string(data, "title")
    required_string(data, "file")
    action = required_string(data, "action").lower()
    if action not in VALID_ACTIONS:
        raise ValueError(
            f"Unsupported action {action!r}; expected one of "
            f"{', '.join(sorted(VALID_ACTIONS))}"
        )
    data["action"] = action

    if action != "delete":
        required_string(data, "prompt")

    depends_on = data.get("depends_on", [])
    if depends_on is not None and not isinstance(depends_on, list):
        raise ValueError("YAML field 'depends_on' must be a list")
    return data


def make_model_prompt(task: dict[str, Any], current_content: str | None) -> str:
    task_prompt = required_string(task, "prompt")
    file_path = required_string(task, "file")

    sections = [
        "You are a code-generation component in an automated task runner.",
        "Implement the task described below.",
        "",
        "TASK METADATA",
        f"ID: {task.get('id', '')}",
        f"Title: {task.get('title', '')}",
        f"Action: {task.get('action', '')}",
        f"Target file: {file_path}",
        "",
        "TASK INSTRUCTIONS",
        task_prompt,
        "",
    ]

    if task["action"] == "modify":
        sections.extend(
            [
                "CURRENT CONTENT OF THE TARGET FILE",
                "The following is the complete current file. Preserve unrelated behavior "
                "and make only the changes required by the task.",
                "----- BEGIN CURRENT FILE -----",
                current_content if current_content is not None else "",
                "----- END CURRENT FILE -----",
                "",
            ]
        )
    elif task["action"] == "create":
        sections.extend(
            [
                "The target file does not exist yet. Generate its complete contents.",
                "",
            ]
        )

    sections.extend(
        [
            "OUTPUT REQUIREMENTS — MUST BE FOLLOWED",
            "- Respond with the complete contents of the target file only.",
            "- Output code only. Do not include Markdown fences, explanations, comments "
            "outside the code, greetings, summaries, or any other prose.",
            "- Do not output a diff or partial snippet.",
            "- Do not include the target file path in the response.",
            "- If the task cannot be implemented, do not fabricate a success response; "
            "return an empty response.",
        ]
    )
    return "\n".join(sections)


def call_ollama(
    prompt: str,
    *,
    model: str,
    base_url: str,
    timeout: int,
    logger: logging.Logger,
) -> str:
    url = base_url.rstrip("/") + "/api/chat"
    payload = {
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "stream": False,
        "options": {
            "temperature": 0,
        },
    }

    logger.info("Calling Ollama URL: %s", url)
    logger.info("Ollama model: %s", model)
    logger.debug("Request payload:\n%s", json.dumps(payload, ensure_ascii=False, indent=2))

    started = time.monotonic()
    response = requests.post(url, json=payload, timeout=timeout)
    elapsed = time.monotonic() - started

    logger.info("Ollama HTTP status: %s; elapsed: %.2f seconds", response.status_code, elapsed)
    logger.debug("Raw Ollama HTTP response:\n%s", response.text)
    response.raise_for_status()

    try:
        data = response.json()
    except ValueError as exc:
        raise RuntimeError("Ollama returned invalid JSON") from exc

    message = data.get("message")
    if not isinstance(message, dict):
        raise RuntimeError(f"Ollama response has no 'message' object: {data!r}")

    content = message.get("content")
    if not isinstance(content, str) or not content.strip():
        raise RuntimeError("Ollama returned an empty model response")

    logger.info(
        "Model response received: %d characters; done_reason=%r",
        len(content),
        data.get("done_reason"),
    )
    logger.debug("Model response content:\n%s", content)
    return content


def normalize_model_output(output: str, logger: logging.Logger) -> str:
    """Remove a single Markdown fence if it wraps the entire response.

    Models often ignore the no-Markdown instruction. Unwrap only a clean outer
    fence; reject prose surrounding the code rather than accidentally writing it.
    """
    text = output.strip()
    if not text:
        raise RuntimeError("Model returned an empty response")

    lines = text.splitlines()
    if lines and lines[0].strip().startswith("```"):
        if len(lines) < 3 or lines[-1].strip() != "```":
            raise RuntimeError("Model output starts with a Markdown fence but has no matching closing fence")
        language = lines[0].strip()[3:].strip()
        body = "\n".join(lines[1:-1]).strip("\n")
        if not body.strip():
            raise RuntimeError("Model returned an empty fenced code block")
        logger.warning(
            "Model wrapped its code in Markdown fences%s; removing the outer fence",
            f" (language={language})" if language else "",
        )
        text = body
    elif text.endswith("```") or "```" in text:
        raise RuntimeError("Model output contains Markdown fences that do not wrap the entire response")

    if not text.strip():
        raise RuntimeError("Model returned an empty response after normalization")
    return text + ("" if text.endswith("\n") else "\n")


def validate_target_path(path_text: str, workspace: Path | None) -> Path:
    target = Path(path_text).expanduser()
    if not target.is_absolute():
        if workspace is None:
            raise ValueError(
                f"Target path {path_text!r} is not absolute; use an absolute path "
                "or pass --workspace"
            )
        target = workspace / target

    # Resolve the parent to normalize it while preserving a not-yet-existing target.
    parent = target.parent.resolve()
    target = parent / target.name

    if workspace is not None:
        workspace_resolved = workspace.resolve()
        try:
            target.relative_to(workspace_resolved)
        except ValueError as exc:
            raise ValueError(
                f"Target {target} is outside allowed workspace {workspace_resolved}"
            ) from exc
    return target


def apply_task(
    task: dict[str, Any],
    *,
    model: str,
    base_url: str,
    timeout: int,
    workspace: Path | None,
    logger: logging.Logger,
) -> None:
    action = task["action"]
    target = validate_target_path(required_string(task, "file"), workspace)
    logger.info("Task ID: %s", task["id"])
    logger.info("Task title: %s", task["title"])
    logger.info("Action: %s", action)
    logger.info("Target: %s", target)

    if action == "delete":
        if not target.exists():
            raise FileNotFoundError(f"Cannot delete: target does not exist: {target}")
        if not target.is_file() or target.is_symlink():
            raise ValueError(f"Delete target must be a regular file, not a symlink: {target}")
        logger.info("Delete action: Ollama will not be called")
        target.unlink()
        logger.info("Deleted target file")
        return

    if target.is_symlink():
        raise ValueError(f"Refusing to replace a symlink: {target}")

    if action == "create":
        if target.exists():
            raise FileExistsError(f"Create action requires a missing target: {target}")
        current_content = None
    else:  # modify
        if not target.exists():
            raise FileNotFoundError(f"Modify action requires an existing target: {target}")
        if not target.is_file():
            raise ValueError(f"Modify target is not a regular file: {target}")
        current_content = target.read_text(encoding="utf-8")
        logger.info("Read current file: %d characters", len(current_content))
        logger.debug("Current file content:\n%s", current_content)

    prompt = make_model_prompt(task, current_content)
    logger.info("Complete model prompt:\n%s", prompt)
    output = call_ollama(
        prompt,
        model=model,
        base_url=base_url,
        timeout=timeout,
        logger=logger,
    )

    # Models frequently wrap source code in a Markdown fence despite the prompt.
    # Unwrap a single outer fence; reject malformed fences or surrounding prose.
    output = normalize_model_output(output, logger)

    # Recheck state immediately before writing to reduce accidental overwrites.
    if action == "create" and target.exists():
        raise FileExistsError(f"Target appeared during generation; refusing to overwrite: {target}")
    if action == "modify" and not target.exists():
        raise FileNotFoundError(f"Target disappeared during generation: {target}")

    target.parent.mkdir(parents=True, exist_ok=True)
    temp_path = target.with_name(f".{target.name}.tmp-{os.getpid()}")
    try:
        with temp_path.open("w", encoding="utf-8", newline="") as handle:
            handle.write(output)
            if not output.endswith("\n"):
                handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        # For modify, replacement is atomic at the filesystem level.
        # For create, the earlier existence check is repeated using hard-link
        # semantics so an existing file is not overwritten.
        if action == "create":
            try:
                os.link(temp_path, target)
            except FileExistsError:
                raise FileExistsError(f"Refusing to overwrite existing file: {target}")
            finally:
                temp_path.unlink(missing_ok=True)
        else:
            os.replace(temp_path, target)
    finally:
        temp_path.unlink(missing_ok=True)

    logger.info("Wrote %s file: %d bytes", action, target.stat().st_size)


def move_task(source: Path, destination_dir: Path, logger: logging.Logger) -> Path:
    destination_dir.mkdir(parents=True, exist_ok=True)
    destination = destination_dir / source.name
    if destination.exists():
        destination = destination_dir / f"{source.stem}-{timestamp()}{source.suffix}"
        logger.warning("Destination already existed; using %s", destination)
    shutil.move(str(source), str(destination))
    logger.info("Moved task YAML to: %s", destination)
    return destination


def process_one(
    task_file: Path,
    *,
    folders: dict[str, Path],
    model: str,
    base_url: str,
    timeout: int,
    workspace: Path | None,
) -> bool:
    log_path = folders["logs"] / f"{task_file.stem}-{timestamp()}.log"
    logger = configure_task_logger(log_path)
    in_progress_file: Path | None = None

    try:
        logger.info("========== TASK START ==========")
        logger.info("Source YAML: %s", task_file)
        in_progress_file = move_task(task_file, folders["in-progress"], logger)

        logger.info("Reading YAML task")
        task = load_task(in_progress_file)
        logger.info("Parsed YAML:\n%s", yaml.safe_dump(task, allow_unicode=True, sort_keys=False))

        apply_task(
            task,
            model=model,
            base_url=base_url,
            timeout=timeout,
            workspace=workspace,
            logger=logger,
        )
        logger.info("========== TASK SUCCESS ==========")
        move_task(in_progress_file, folders["done"], logger)
        return True

    except Exception as exc:
        logger.exception("========== TASK FAILURE: %s: %s ==========", type(exc).__name__, exc)
        if in_progress_file is not None and in_progress_file.exists():
            try:
                move_task(in_progress_file, folders["failure"], logger)
            except Exception:
                logger.exception("Could not move failed task YAML to failure/")
        return False
    finally:
        for handler in list(logger.handlers):
            handler.flush()
            handler.close()
            logger.removeHandler(handler)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Run YAML code-editing tasks sequentially through Ollama."
    )
    parser.add_argument("--tasks-dir", type=Path, default=Path("."),
                        help="Directory containing todo/, in-progress/, done/, failure/, logs/ (default: current directory)")
    parser.add_argument("--model", default=os.environ.get("OLLAMA_MODEL", DEFAULT_MODEL))
    parser.add_argument("--ollama-url", default=os.environ.get("OLLAMA_BASE_URL", DEFAULT_OLLAMA_URL))
    parser.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT,
                        help=f"Per-request timeout in seconds (default: {DEFAULT_TIMEOUT})")
    parser.add_argument("--workspace", type=Path, default=None,
                        help="Optional allowed workspace; target files must remain inside it")
    parser.add_argument("--poll", type=float, default=0,
                        help="If > 0, poll todo/ every this many seconds until interrupted")
    args = parser.parse_args()

    root = args.tasks_dir.expanduser().resolve()
    folders = {
        "todo": root / "todo",
        "in-progress": root / "in-progress",
        "done": root / "done",
        "failure": root / "failure",
        "logs": root / "logs",
    }
    for directory in folders.values():
        directory.mkdir(parents=True, exist_ok=True)

    workspace = args.workspace.expanduser().resolve() if args.workspace else None
    if args.timeout <= 0:
        parser.error("--timeout must be greater than zero")
    if args.poll < 0:
        parser.error("--poll cannot be negative")

    print(f"Task directory: {root}")
    print(f"Ollama: {args.ollama_url} (model={args.model})")
    print(f"Timeout: {args.timeout}s")

    failures = 0
    try:
        while True:
            task_files = sorted(
                (p for p in folders["todo"].iterdir()
                 if p.is_file() and p.suffix.lower() in {".yaml", ".yml"}),
                key=lambda p: p.name,
            )
            if not task_files:
                if args.poll > 0:
                    time.sleep(args.poll)
                    continue
                break

            for task_file in task_files:
                print(f"[{timestamp()}] Processing {task_file.name}", flush=True)
                ok = process_one(
                    task_file,
                    folders=folders,
                    model=args.model,
                    base_url=args.ollama_url,
                    timeout=args.timeout,
                    workspace=workspace,
                )
                if ok:
                    print(f"[{timestamp()}] DONE: {task_file.name}", flush=True)
                else:
                    failures += 1
                    print(f"[{timestamp()}] FAILURE: {task_file.name}", flush=True)

            if args.poll <= 0:
                break
    except KeyboardInterrupt:
        print("\nInterrupted by user.", file=sys.stderr)
        return 130

    print(f"Finished. Failed tasks in this run: {failures}")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
