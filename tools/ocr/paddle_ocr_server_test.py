from __future__ import annotations

import sys
import threading
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


TOOLS_OCR = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS_OCR))

import paddle_ocr_server  # noqa: E402


class RecordingRunner:
    def __init__(self, returncodes: list[int] | None = None) -> None:
        self.returncodes = list(returncodes or [0])
        self.calls: list[list[str]] = []

    def __call__(self, command: list[str], **_: object) -> SimpleNamespace:
        self.calls.append(command)
        returncode = self.returncodes.pop(0) if self.returncodes else 0
        return SimpleNamespace(
            returncode=returncode,
            stdout="",
            stderr="permission denied" if returncode else "",
        )


class FakeTimer:
    instances: list["FakeTimer"] = []

    def __init__(self, delay: float, callback: object) -> None:
        self.delay = delay
        self.callback = callback
        self.daemon = False
        self.started = False
        self.cancelled = False
        self.instances.append(self)

    def start(self) -> None:
        self.started = True

    def cancel(self) -> None:
        self.cancelled = True


class GpuLeaseControllerTests(unittest.TestCase):
    def setUp(self) -> None:
        FakeTimer.instances.clear()

    def controller(
        self,
        runner: RecordingRunner,
        release_delay_seconds: float = 0,
    ) -> paddle_ocr_server.GpuLeaseController:
        return paddle_ocr_server.GpuLeaseController(
            acquire_unit="recitopia-ocr-gpu-acquire.service",
            release_unit="recitopia-ocr-gpu-release.service",
            systemctl_bin="/run/current-system/sw/bin/systemctl",
            release_delay_seconds=release_delay_seconds,
            runner=runner,
            timer_factory=FakeTimer,
        )

    def test_acquire_and_immediate_release_use_only_helper_units(self) -> None:
        runner = RecordingRunner([0, 0])
        controller = self.controller(runner)

        controller.acquire()
        self.assertTrue(controller.held)
        controller.schedule_release()

        self.assertFalse(controller.held)
        self.assertEqual(
            [
                [
                    "/run/current-system/sw/bin/systemctl",
                    "--no-ask-password",
                    "start",
                    "recitopia-ocr-gpu-acquire.service",
                ],
                [
                    "/run/current-system/sw/bin/systemctl",
                    "--no-ask-password",
                    "start",
                    "recitopia-ocr-gpu-release.service",
                ],
            ],
            runner.calls,
        )

    def test_next_batch_reuses_lease_and_cancels_delayed_release(self) -> None:
        runner = RecordingRunner()
        controller = self.controller(runner, release_delay_seconds=20)

        controller.acquire()
        controller.schedule_release()
        timer = FakeTimer.instances[0]
        self.assertTrue(timer.started)
        self.assertEqual(20, timer.delay)

        controller.acquire()

        self.assertTrue(timer.cancelled)
        self.assertTrue(controller.held)
        self.assertEqual(1, len(runner.calls))

    def test_acquire_failure_prevents_ocr_from_running_without_gpu_lease(self) -> None:
        runner = RecordingRunner([1])
        controller = self.controller(runner)

        with self.assertRaisesRegex(paddle_ocr_server.OcrUnavailable, "permission denied"):
            controller.acquire()

        self.assertFalse(controller.held)

    def test_invalid_helper_unit_disables_lease(self) -> None:
        runner = RecordingRunner()
        controller = paddle_ocr_server.GpuLeaseController(
            acquire_unit="../../arbitrary",
            release_unit="recitopia-ocr-gpu-release.service",
            systemctl_bin="systemctl",
            release_delay_seconds=0,
            runner=runner,
        )

        controller.acquire()

        self.assertFalse(controller.configured)
        self.assertEqual([], runner.calls)


class OcrBatchGpuLeaseTests(unittest.TestCase):
    def test_batch_schedules_release_when_page_processing_fails(self) -> None:
        class LeaseSpy:
            configured = True
            held = False

            def __init__(self) -> None:
                self.acquired = 0
                self.scheduled = 0

            def acquire(self) -> None:
                self.acquired += 1

            def schedule_release(self) -> None:
                self.scheduled += 1

        lease = LeaseSpy()
        with (
            mock.patch.object(paddle_ocr_server, "_gpu_lease", lease),
            mock.patch.object(paddle_ocr_server, "_ocr_lock", threading.Lock()),
            mock.patch.object(paddle_ocr_server, "_ocr", object()),
            mock.patch.object(paddle_ocr_server, "_api_mode", "paddleocr3"),
            mock.patch.object(paddle_ocr_server, "_version", "test"),
            mock.patch.object(paddle_ocr_server, "run_ocr", side_effect=RuntimeError("test failure")),
            mock.patch.object(paddle_ocr_server, "release_unused_gpu_memory") as release_cache,
        ):
            result = paddle_ocr_server.ocr_batch_payload(
                {"pages": [{"id": "page-1", "imagePath": "/tmp/page-1.jpg"}]}
            )

        self.assertEqual(1, lease.acquired)
        self.assertEqual(1, lease.scheduled)
        release_cache.assert_called_once_with()
        self.assertFalse(result["results"][0]["ok"])


if __name__ == "__main__":
    unittest.main()
