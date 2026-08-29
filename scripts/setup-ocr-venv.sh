#!/usr/bin/env bash
set -uo pipefail

VENV="${1:-${RECITOPIA_OCR_VENV:-$HOME/.local/share/recitopia/ocr-venv}}"
PY="${PYBIN:-python3.12}"
CUDA_INDEX="${PADDLE_INDEX:-https://www.paddlepaddle.org.cn/packages/stable/cu118/}"
PADDLE_VERSION="${PADDLE_VERSION:-3.2.0}"

die() { printf 'setup-ocr-venv: %s\n' "$*" >&2; exit 1; }
log() { printf '==> %s\n' "$*" >&2; }

command -v "$PY" >/dev/null 2>&1 \
  || die "need $PY. PaddlePaddle publishes no wheels for 3.13+; set PYBIN to a 3.12 interpreter."

version="$("$PY" -c 'import sys; print("%d.%d" % sys.version_info[:2])')"
case "$version" in
  3.9|3.10|3.11|3.12) ;;
  *) die "python $version has no PaddlePaddle wheels; use 3.12" ;;
esac

[ ! -e "$VENV" ] || die "$VENV already exists"

log "creating venv at $VENV (python $version)"
mkdir -p "$(dirname "$VENV")"
"$PY" -m venv "$VENV" || die "venv creation failed"
"$VENV/bin/python" -m pip install -q -U pip wheel || die "pip bootstrap failed"

log "installing paddlepaddle-gpu $PADDLE_VERSION"
if "$VENV/bin/python" -m pip install "paddlepaddle-gpu==$PADDLE_VERSION" -i "$CUDA_INDEX" \
     >/tmp/recitopia-paddle.log 2>&1; then
  log "gpu build installed"
else
  log "gpu build unavailable, falling back to cpu"
  tail -3 /tmp/recitopia-paddle.log >&2
  "$VENV/bin/python" -m pip install paddlepaddle >/tmp/recitopia-paddle-cpu.log 2>&1 \
    || { tail -5 /tmp/recitopia-paddle-cpu.log >&2; die "paddlepaddle install failed"; }
fi

log "installing paddleocr and support libraries"
"$VENV/bin/python" -m pip install paddleocr numpy pillow opencv-python-headless fastapi uvicorn \
  >/tmp/recitopia-ocr-deps.log 2>&1 \
  || { tail -5 /tmp/recitopia-ocr-deps.log >&2; die "paddleocr install failed"; }

ldpath=""
if [ -d /run/opengl-driver/lib ]; then
  ldpath="/run/opengl-driver/lib:/run/current-system/sw/lib:/run/current-system/sw/share/nix-ld/lib"
fi

log "verifying"
if ! LD_LIBRARY_PATH="$ldpath${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
     "$VENV/bin/python" -c 'import paddle, paddleocr; print("paddle", paddle.__version__, "cuda", paddle.device.is_compiled_with_cuda(), "paddleocr", paddleocr.__version__)'; then
  die "paddle imported but failed to load. On NixOS libcuda.so.1 lives in
/run/opengl-driver/lib; export LD_LIBRARY_PATH before running the OCR server."
fi

cat <<EOF

RECITOPIA_OCR_PYTHON=$VENV/bin/python
EOF

if [ -n "$ldpath" ]; then
  cat <<EOF
LD_LIBRARY_PATH=$ldpath

This host keeps its GPU driver outside the default linker path, so the OCR
process needs that LD_LIBRARY_PATH. The NixOS module sets it already.
EOF
fi
