{
  duckdb,
  lib,
  pkg-config,
  rustPlatform,
}:

rustPlatform.buildRustPackage {
  pname = "recitopia-api-rs";
  version = "0.1.0";

  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: type:
      let
        rel = lib.removePrefix (toString ./. + "/") (toString path);
      in
        !(lib.hasPrefix "target/" rel || rel == "target");
  };

  cargoLock.lockFile = ./Cargo.lock;
  buildNoDefaultFeatures = true;
  buildFeatures = [ "system-duckdb" ];

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ duckdb ];

  DUCKDB_INCLUDE_DIR = "${lib.getDev duckdb}/include";
  DUCKDB_LIB_DIR = "${lib.getLib duckdb}/lib";

  postInstall = ''
    mkdir -p "$out/share/recitopia/tools/ocr" "$out/share/recitopia/tools/ml"
    cp ${../../tools/ocr/paddle_ocr.py} "$out/share/recitopia/tools/ocr/paddle_ocr.py"
    cp ${../../tools/ocr/paddle_ocr_server.py} "$out/share/recitopia/tools/ocr/paddle_ocr_server.py"
    cp ${../../tools/ocr/page_crop.py} "$out/share/recitopia/tools/ocr/page_crop.py"
    cp ${../../tools/ml/deepseek_mapper.py} "$out/share/recitopia/tools/ml/deepseek_mapper.py"
    cp ${../../tools/ml/deepseek_cookbook_mapper.py} "$out/share/recitopia/tools/ml/deepseek_cookbook_mapper.py"
  '';

  meta = {
    description = "Recitopia Rust API service";
    mainProgram = "recitopia-api-rs";
    platforms = lib.platforms.linux;
  };
}
