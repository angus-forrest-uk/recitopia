{ config, lib, pkgs, ... }:

let
  cfg = config.services.recitopia-api-rust-shadow;
  ocrLibraryPath = "${lib.makeLibraryPath [ pkgs.stdenv.cc.cc.lib ]}:/run/opengl-driver/lib:/run/current-system/sw/lib:/run/current-system/sw/share/nix-ld/lib";
in
{
  options.services.recitopia-api-rust-shadow = {
    enable = lib.mkEnableOption "the Recitopia Rust shadow API";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Rust Recitopia API package to run.";
    };

    host = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      description = "Address for the shadow listener.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 8079;
      description = "Port for the shadow listener.";
    };

    databasePath = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/recitopia-rust-shadow/recitopia.duckdb";
      description = "Copied DuckDB database used by the shadow service.";
    };

    storeMode = lib.mkOption {
      type = lib.types.enum [ "read-only" "read-write" ];
      default = "read-only";
      description = "Shadow database access mode. Never use read-write against the live Zig database.";
    };

    importDir = lib.mkOption {
      type = lib.types.path;
      default = "/mnt/raid/recitopia/rust-shadow/imports";
      description = "Independent asset and diagnostic directory for Rust shadow jobs.";
    };

    ocrServerUrl = lib.mkOption {
      type = lib.types.str;
      default = "http://127.0.0.1:8078";
      description = "Existing Recitopia PaddleOCR service URL.";
    };

    ocrPython = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = "/var/lib/recitopia/ocr-venv/bin/python";
      description = "Optional Python executable with PaddleOCR installed, used only if the shared OCR server is unavailable.";
    };

    pipelineConcurrency = lib.mkOption {
      type = lib.types.ints.between 1 16;
      default = 2;
      description = "Maximum concurrent Rust import/diagnostic jobs.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "recitopia";
      description = "User that can read the copied database and source images.";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "recitopia";
      description = "Group for the shadow service.";
    };

    environmentFiles = lib.mkOption {
      type = lib.types.listOf lib.types.path;
      default = [ ];
      example = [ "/etc/recitopia/llm" ];
      description = "Optional environment files, including the LLM API key.";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open the shadow port on one interface.";
    };

    firewallInterface = lib.mkOption {
      type = lib.types.str;
      default = "tailscale0";
      description = "Interface on which to expose the shadow port.";
    };

    extraEnvironment = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = "Additional environment variables for controlled shadow experiments.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.storeMode != "read-write"
          || toString cfg.databasePath != "/var/lib/recitopia/recitopia.duckdb";
        message = "The Rust shadow service must not open the live Zig database read-write.";
      }
    ];

    systemd.tmpfiles.rules = [
      "d ${toString (builtins.dirOf cfg.databasePath)} 0750 ${cfg.user} ${cfg.group} -"
      "d ${toString cfg.importDir} 0750 ${cfg.user} ${cfg.group} -"
    ];

    systemd.services.recitopia-api-rust-shadow = {
      description = "Recitopia Rust API shadow";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" "recitopia-ocr.service" ];
      wants = [ "network-online.target" ];
      environment = {
        RECITOPIA_RUST_API_HOST = cfg.host;
        RECITOPIA_RUST_API_PORT = toString cfg.port;
        RECITOPIA_RUST_DB_PATH = toString cfg.databasePath;
        RECITOPIA_RUST_STORE_MODE = cfg.storeMode;
        RECITOPIA_RUST_IMPORT_DIR = toString cfg.importDir;
        RECITOPIA_RUST_OCR_SERVER_URL = cfg.ocrServerUrl;
        RECITOPIA_RUST_OCR_SCRIPT = "${cfg.package}/share/recitopia/tools/ocr/paddle_ocr.py";
        RECITOPIA_RUST_PIPELINE_CONCURRENCY = toString cfg.pipelineConcurrency;
        RECITOPIA_RUST_IMAGE_CONVERT_BIN = "${pkgs.imagemagick}/bin/magick";
        RECITOPIA_RUST_LLM_PYTHON = "${pkgs.python3}/bin/python3";
        RECITOPIA_RUST_LLM_COOKBOOK_SCRIPT = "${cfg.package}/share/recitopia/tools/ml/llm_cookbook_mapper.py";
        RECITOPIA_RUST_LLM_RECIPE_SCRIPT = "${cfg.package}/share/recitopia/tools/ml/llm_mapper.py";
        RUST_LOG = "recitopia_api_rs=info";
      } // lib.optionalAttrs (cfg.ocrPython != null) {
        LD_LIBRARY_PATH = ocrLibraryPath;
        RECITOPIA_RUST_OCR_PYTHON = toString (pkgs.writeShellScript "recitopia-rust-shadow-ocr-python" ''
          export LD_LIBRARY_PATH="${ocrLibraryPath}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
          exec ${toString cfg.ocrPython} "$@"
        '');
      } // cfg.extraEnvironment;

      serviceConfig = {
        Type = "simple";
        ExecStart = lib.getExe cfg.package;
        User = cfg.user;
        Group = cfg.group;
        WorkingDirectory = toString (builtins.dirOf cfg.databasePath);
        EnvironmentFile = map (path: "-${toString path}") cfg.environmentFiles;
        Restart = "on-failure";
        RestartSec = "5s";
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        ReadWritePaths = [
          (toString (builtins.dirOf cfg.databasePath))
          (toString cfg.importDir)
        ];
        ReadOnlyPaths = [
          "-/mnt/raid/recitopia/imports"
          "-/var/lib/recitopia/imports"
        ];
        UMask = "0027";
      };
    };

    networking.firewall = lib.mkIf cfg.openFirewall {
      interfaces.${cfg.firewallInterface}.allowedTCPPorts = [ cfg.port ];
    };
  };
}
