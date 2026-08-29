#!/usr/bin/env python3
"""Long-lived PaddleOCR service for Recitopia.

The API can still run paddle_ocr.py as a subprocess, but cookbook imports are
much faster when the model is loaded once and reused through this localhost API.

This server intentionally starts even when PaddleOCR cannot be loaded. NixOS
switches should not fail just because the optional OCR venv is temporarily
missing a package or GPU library. In that case /health reports the load error
and OCR requests return 503, which lets the API fall back to the subprocess path.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import threading
import time
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

from paddle_ocr import load_ocr, run_ocr


_ocr: Any | None = None
_api_mode = ""
_version = ""
_load_error: str | None = None
_ocr_lock = threading.Lock()


class OcrUnavailable(RuntimeError):
    pass


def log_event(level: str, event: str, **fields: Any) -> None:
    payload = {"level": level, "event": event, **fields}
    print(json.dumps(payload, ensure_ascii=False, sort_keys=True), flush=True)


SYSTEMD_UNIT_RE = re.compile(r"^[A-Za-z0-9_.@:-]+\.service$")


class GpuLeaseController:
    """Temporarily yield a shared GPU service while OCR batches run.

    The configured systemd units are root-owned helpers. The OCR process only
    receives permission to start those helpers; it cannot manage arbitrary
    services directly.
    """

    def __init__(
        self,
        acquire_unit: str | None,
        release_unit: str | None,
        systemctl_bin: str,
        release_delay_seconds: float,
        runner: Any = subprocess.run,
        timer_factory: Any = threading.Timer,
    ) -> None:
        self.acquire_unit = acquire_unit or ""
        self.release_unit = release_unit or ""
        self.systemctl_bin = systemctl_bin
        self.release_delay_seconds = max(0.0, release_delay_seconds)
        self._runner = runner
        self._timer_factory = timer_factory
        self._lock = threading.Lock()
        self._release_timer: Any | None = None
        self._held = False

        units = [unit for unit in (self.acquire_unit, self.release_unit) if unit]
        if units and (len(units) != 2 or any(not SYSTEMD_UNIT_RE.fullmatch(unit) for unit in units)):
            log_event("ERROR", "ocr_gpu_lease_configuration_invalid", units=units)
            self.acquire_unit = ""
            self.release_unit = ""

    @property
    def configured(self) -> bool:
        return bool(self.acquire_unit and self.release_unit)

    @property
    def held(self) -> bool:
        with self._lock:
            return self._held

    def acquire(self) -> None:
        if not self.configured:
            return
        with self._lock:
            if self._release_timer is not None:
                self._release_timer.cancel()
                self._release_timer = None
            if self._held:
                log_event("INFO", "ocr_gpu_lease_reused")
                return

            log_event("INFO", "ocr_gpu_lease_acquire_start", unit=self.acquire_unit)
            self._start_helper(self.acquire_unit, "acquire")
            self._held = True
            log_event("INFO", "ocr_gpu_lease_acquired", unit=self.acquire_unit)

    def schedule_release(self) -> None:
        if not self.configured:
            return
        release_now = False
        with self._lock:
            if not self._held:
                return
            if self._release_timer is not None:
                self._release_timer.cancel()
            if self.release_delay_seconds == 0:
                self._release_timer = None
                release_now = True
            else:
                timer = self._timer_factory(self.release_delay_seconds, self.release)
                timer.daemon = True
                timer.start()
                self._release_timer = timer
                log_event(
                    "INFO",
                    "ocr_gpu_lease_release_scheduled",
                    delay_seconds=self.release_delay_seconds,
                )
        if release_now:
            self.release()

    def release(self) -> None:
        if not self.configured:
            return
        with self._lock:
            if self._release_timer is not None:
                self._release_timer.cancel()
                self._release_timer = None
            if not self._held:
                return

            log_event("INFO", "ocr_gpu_lease_release_start", unit=self.release_unit)
            try:
                self._start_helper(self.release_unit, "release")
            except OcrUnavailable as exc:
                # Keep the lease marked as held. The NixOS ExecStopPost
                # failsafe can retry restoration if this process exits.
                log_event("ERROR", "ocr_gpu_lease_release_failed", error=str(exc))
                return
            self._held = False
            log_event("INFO", "ocr_gpu_lease_released", unit=self.release_unit)

    def _start_helper(self, unit: str, action: str) -> None:
        try:
            result = self._runner(
                [self.systemctl_bin, "--no-ask-password", "start", unit],
                capture_output=True,
                text=True,
                timeout=60,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise OcrUnavailable(f"GPU lease {action} helper failed: {type(exc).__name__}: {exc}") from exc
        if result.returncode == 0:
            return
        detail = (result.stderr or result.stdout or "unknown systemctl error").strip().replace("\n", " ")
        raise OcrUnavailable(f"GPU lease {action} helper failed ({result.returncode}): {detail[:500]}")


def gpu_release_delay_seconds() -> float:
    raw = os.getenv("RECITOPIA_OCR_GPU_RELEASE_DELAY_SECONDS", "20")
    try:
        return min(600.0, max(0.0, float(raw)))
    except ValueError:
        log_event("WARN", "ocr_gpu_release_delay_invalid", value=raw, fallback=20)
        return 20.0


_gpu_lease = GpuLeaseController(
    acquire_unit=os.getenv("RECITOPIA_OCR_GPU_ACQUIRE_UNIT"),
    release_unit=os.getenv("RECITOPIA_OCR_GPU_RELEASE_UNIT"),
    systemctl_bin=os.getenv("RECITOPIA_OCR_SYSTEMCTL_BIN", "systemctl"),
    release_delay_seconds=gpu_release_delay_seconds(),
)


def engine_name() -> str:
    return f"paddleocr:{_version or 'unknown'}:{_api_mode or 'unknown'}"


def ensure_ocr_loaded() -> None:
    global _ocr, _api_mode, _version, _load_error
    if _ocr is not None:
        return
    if _load_error is not None:
        raise OcrUnavailable(_load_error)
    try:
        _ocr, _api_mode, _version = load_ocr()
    except Exception as exc:  # pragma: no cover - exercised on the server.
        _load_error = f"{type(exc).__name__}: {exc}"
        raise OcrUnavailable(_load_error) from exc


def warmup_ocr() -> None:
    try:
        with _ocr_lock:
            ensure_ocr_loaded()
    except OcrUnavailable as exc:
        log_event("WARN", "ocr_warmup_failed", error=str(exc))
    else:
        log_event("INFO", "ocr_warmup_complete", engine=engine_name())


def health_payload() -> dict[str, Any]:
    return {
        "ok": _ocr is not None and _load_error is None,
        "engine": engine_name() if _ocr is not None else None,
        "error": _load_error,
        "busy": _ocr_lock.locked(),
        "gpuLeaseConfigured": _gpu_lease.configured,
        "gpuLeaseHeld": _gpu_lease.held,
    }


def release_unused_gpu_memory() -> None:
    if not _gpu_lease.configured:
        return
    try:
        import paddle

        empty_cache = getattr(getattr(paddle.device, "cuda", None), "empty_cache", None)
        if callable(empty_cache):
            empty_cache()
            log_event("INFO", "ocr_gpu_cache_released")
    except Exception as exc:  # pragma: no cover - depends on the server CUDA runtime.
        log_event("WARN", "ocr_gpu_cache_release_failed", error=f"{type(exc).__name__}: {exc}")


def ocr_batch_payload(body: dict[str, Any]) -> dict[str, Any]:
    pages = body.get("pages")
    if not isinstance(pages, list) or len(pages) == 0:
        raise ValueError("pages must be a non-empty list")

    queued_at = time.monotonic()
    log_event("INFO", "ocr_batch_queued", pages=len(pages), busy=_ocr_lock.locked())
    results: list[dict[str, Any]] = []
    with _ocr_lock:
        started_at = time.monotonic()
        wait_ms = int((started_at - queued_at) * 1000)
        _gpu_lease.acquire()
        try:
            ensure_ocr_loaded()
            engine = engine_name()
            log_event("INFO", "ocr_batch_started", pages=len(pages), wait_ms=wait_ms, engine=engine)
            for page in pages:
                if not isinstance(page, dict):
                    results.append(
                        {
                            "id": "",
                            "imagePath": "",
                            "ok": False,
                            "engine": engine,
                            "error": "ValueError: page must be an object",
                        }
                    )
                    log_event("WARN", "ocr_page_invalid", error="page must be an object")
                    continue

                page_id = str(page.get("id") or "")
                image_path = str(page.get("imagePath") or "")
                try:
                    if not page_id or not image_path:
                        raise ValueError("page id and imagePath are required")
                    payload = run_ocr(Path(image_path), _ocr, _api_mode, _version)
                    results.append(
                        {
                            "id": page_id,
                            "imagePath": image_path,
                            "ok": True,
                            "engine": str(payload.get("engine") or engine),
                            "text": str(payload.get("text") or ""),
                            "printedPageNumber": payload.get("printedPageNumber"),
                            "printedPageNumberSide": payload.get("printedPageNumberSide"),
                            "raw": payload,
                        }
                    )
                except Exception as exc:
                    error = f"{type(exc).__name__}: {exc}"
                    results.append(
                        {
                            "id": page_id,
                            "imagePath": image_path,
                            "ok": False,
                            "engine": engine,
                            "error": error,
                        }
                    )
                    log_event("ERROR", "ocr_page_failed", page_id=page_id, image_path=image_path, error=error)
            duration_ms = int((time.monotonic() - started_at) * 1000)
            failed = sum(1 for result in results if not result.get("ok"))
            log_event("INFO", "ocr_batch_complete", pages=len(pages), failed=failed, duration_ms=duration_ms)
        finally:
            release_unused_gpu_memory()
            _gpu_lease.schedule_release()
    return {"engine": engine, "results": results}


def ocr_one_payload(body: dict[str, Any]) -> dict[str, Any]:
    return ocr_batch_payload({"pages": [body]})["results"][0]


def create_fastapi_app() -> Any | None:
    try:
        from fastapi import FastAPI, HTTPException
        from pydantic import BaseModel, Field
    except Exception as exc:  # pragma: no cover - depends on the server venv.
        log_event("WARN", "fastapi_unavailable", error=f"{type(exc).__name__}: {exc}")
        return None

    class OcrPageRequest(BaseModel):
        id: str = Field(min_length=1)
        imagePath: str = Field(min_length=1)

    class OcrBatchRequest(BaseModel):
        pages: list[OcrPageRequest] = Field(min_length=1)

    def model_to_dict(value: BaseModel) -> dict[str, Any]:
        if hasattr(value, "model_dump"):
            return value.model_dump()
        return value.dict()

    app = FastAPI(title="Recitopia OCR", version="0.1.0")

    @app.on_event("startup")
    def startup() -> None:
        warmup_ocr()

    @app.get("/health")
    def health() -> dict[str, Any]:
        return health_payload()

    @app.post("/ocr/batch")
    def ocr_batch(request: OcrBatchRequest) -> dict[str, Any]:
        try:
            return ocr_batch_payload({"pages": [model_to_dict(page) for page in request.pages]})
        except OcrUnavailable as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc

    @app.post("/ocr")
    def ocr_one(page: OcrPageRequest) -> dict[str, Any]:
        try:
            return ocr_one_payload(model_to_dict(page))
        except OcrUnavailable as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc

    return app


class StdlibOcrHandler(BaseHTTPRequestHandler):
    server_version = "RecitopiaOCR/0.1"

    def do_GET(self) -> None:  # noqa: N802 - stdlib handler API.
        if self.path != "/health":
            self.write_json({"error": "not found"}, HTTPStatus.NOT_FOUND)
            return
        self.write_json(health_payload())

    def do_POST(self) -> None:  # noqa: N802 - stdlib handler API.
        try:
            body = self.read_json_body()
            if self.path == "/ocr/batch":
                self.write_json(ocr_batch_payload(body))
            elif self.path == "/ocr":
                self.write_json(ocr_one_payload(body))
            else:
                self.write_json({"error": "not found"}, HTTPStatus.NOT_FOUND)
        except OcrUnavailable as exc:
            self.write_json({"error": str(exc)}, HTTPStatus.SERVICE_UNAVAILABLE)
        except Exception as exc:
            self.write_json({"error": f"{type(exc).__name__}: {exc}"}, HTTPStatus.BAD_REQUEST)

    def read_json_body(self) -> dict[str, Any]:
        length = int(self.headers.get("content-length", "0"))
        raw = self.rfile.read(length)
        value = json.loads(raw.decode("utf-8"))
        if not isinstance(value, dict):
            raise ValueError("request body must be a JSON object")
        return value

    def write_json(self, payload: dict[str, Any], status: HTTPStatus = HTTPStatus.OK) -> None:
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status.value)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args: Any) -> None:
        log_event("INFO", "ocr_http_request", client=self.address_string(), message=format % args)


def main() -> int:
    host = os.getenv("RECITOPIA_OCR_SERVER_HOST", "127.0.0.1")
    port = int(os.getenv("RECITOPIA_OCR_SERVER_PORT", "8078"))
    app = create_fastapi_app()

    if app is not None:
        try:
            import uvicorn
        except Exception as exc:  # pragma: no cover - depends on the server venv.
            log_event("WARN", "uvicorn_unavailable", error=f"{type(exc).__name__}: {exc}")
        else:
            log_event("INFO", "ocr_server_listening", host=host, port=port, server="uvicorn")
            uvicorn.run(app, host=host, port=port, log_level=os.getenv("RECITOPIA_OCR_LOG_LEVEL", "info"))
            return 0

    warmup_ocr()
    server = ThreadingHTTPServer((host, port), StdlibOcrHandler)
    log_event("INFO", "ocr_server_listening", host=host, port=port, server="stdlib")
    server.serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
