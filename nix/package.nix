{
  lib,
  stdenv,
  duckdb,
  makeWrapper,
  zig,
  zig_0_16 ? zig,
}:

stdenv.mkDerivation {
  pname = "recitopia-api";
  version = "0.1.0";

  src = lib.cleanSourceWith {
    src = ../.;
    filter = path: type:
      let
        root = toString ../.;
        pathString = toString path;
        rel = if pathString == root then "" else lib.removePrefix (root + "/") pathString;
        isCache =
          lib.hasInfix "/.zig-cache/" rel
          || lib.hasInfix "/zig-out/" rel
          || lib.hasSuffix "/.zig-cache" rel
          || lib.hasSuffix "/zig-out" rel;
        isAllowedDirectory =
          rel == ""
          || rel == "apps"
          || rel == "apps/api"
          || lib.hasPrefix "apps/api/src" rel
          || lib.hasPrefix "apps/api/zig-pkg" rel
          || rel == "tools"
          || lib.hasPrefix "tools/ocr" rel
          || lib.hasPrefix "tools/ml" rel
          || lib.hasPrefix "nix" rel
          || lib.hasPrefix "docs" rel;
        isAllowedFile =
          rel == "apps/api/build.zig"
          || rel == "apps/api/build.zig.zon"
          || lib.hasPrefix "apps/api/src/" rel
          || lib.hasPrefix "apps/api/zig-pkg/" rel
          || lib.hasPrefix "tools/ocr/" rel
          || lib.hasPrefix "tools/ml/" rel
          || lib.hasPrefix "nix/" rel
          || rel == "docs/the server-api-deploy.md"
          || rel == "flake.nix"
          || rel == "README.md"
          || rel == "LLM.md";
      in
        !isCache && (
          if type == "directory" then isAllowedDirectory else isAllowedFile
        );
  };

  nativeBuildInputs = [
    zig_0_16
    makeWrapper
  ];

  buildInputs = [
    duckdb
  ];

  dontConfigure = true;

  buildPhase = ''
    runHook preBuild

    export ZIG_GLOBAL_CACHE_DIR="$TMPDIR/zig-global-cache"
    export ZIG_LOCAL_CACHE_DIR="$TMPDIR/zig-local-cache"

    cp nix/api-local-build.zig.zon apps/api/build.zig.zon
    cp nix/zuckdb-local-build.zig.zon \
      apps/api/zig-pkg/zuckdb-0.0.0-DpSXVN_nBgCGmyd-fU3yPY_C983kZJ2DCEnldLGIuJSk/build.zig.zon

    cd apps/api
    zig build -Doptimize=ReleaseSafe -Dsystem-libduckdb=true --summary all

    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall

    mkdir -p "$out/bin"
    cp zig-out/bin/recitopia-api "$out/bin/recitopia-api"

    mkdir -p "$out/share/recitopia/tools/ocr" "$out/share/recitopia/tools/ml"
    cp ../../tools/ocr/paddle_ocr.py "$out/share/recitopia/tools/ocr/paddle_ocr.py"
    cp ../../tools/ocr/paddle_ocr_server.py "$out/share/recitopia/tools/ocr/paddle_ocr_server.py"
    cp ../../tools/ocr/page_crop.py "$out/share/recitopia/tools/ocr/page_crop.py"
    cp ../../tools/ml/deepseek_mapper.py "$out/share/recitopia/tools/ml/deepseek_mapper.py"
    cp ../../tools/ml/deepseek_cookbook_mapper.py "$out/share/recitopia/tools/ml/deepseek_cookbook_mapper.py"

    wrapProgram "$out/bin/recitopia-api" \
      --set-default RECITOPIA_OCR_SCRIPT "$out/share/recitopia/tools/ocr/paddle_ocr.py" \
      --set-default RECITOPIA_PAGE_CROP_SCRIPT "$out/share/recitopia/tools/ocr/page_crop.py" \
      --set-default RECITOPIA_DEEPSEEK_SCRIPT "$out/share/recitopia/tools/ml/deepseek_mapper.py" \
      --set-default RECITOPIA_DEEPSEEK_COOKBOOK_SCRIPT "$out/share/recitopia/tools/ml/deepseek_cookbook_mapper.py"

    runHook postInstall
  '';

  meta = {
    description = "Recitopia Zig API service";
    mainProgram = "recitopia-api";
    platforms = lib.platforms.linux;
  };
}
