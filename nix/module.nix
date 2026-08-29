{ config, lib, pkgs, ... }:

let
  cfg = config.services.recitopia-api;
  optionalEnvironmentFile = path: "-${toString path}";
  environmentFiles =
    (map optionalEnvironmentFile cfg.environmentFiles)
    ++ lib.optional (cfg.environmentFile != null) (optionalEnvironmentFile cfg.environmentFile);
  ocrLibraryPath = "${lib.makeLibraryPath [ pkgs.stdenv.cc.cc.lib ]}:/run/opengl-driver/lib:/run/current-system/sw/lib:/run/current-system/sw/share/nix-ld/lib";
  ocrGpuPauseUnit = if cfg.ocrGpuPauseUnit == null then "" else cfg.ocrGpuPauseUnit;
  ocrGpuLeaseEnabled = cfg.ocrPython != null && cfg.ocrServerEnable && cfg.ocrGpuPauseUnit != null;
  ocrGpuAcquireUnit = "recitopia-ocr-gpu-acquire.service";
  ocrGpuReleaseUnit = "recitopia-ocr-gpu-release.service";
  dynamicStorageEnabled = cfg.preferredDataDir != null;
  runtimeStorageDir = "/run/recitopia";
  runtimeStorageEnvFile = "${runtimeStorageDir}/storage.env";
  preferredDataDir = if cfg.preferredDataDir == null then "" else toString cfg.preferredDataDir;
  preferredDataMountPoint = if cfg.preferredDataMountPoint == null then "" else toString cfg.preferredDataMountPoint;
  preferredDataParent = if cfg.preferredDataDir == null then "" else builtins.dirOf preferredDataDir;
  scratchDataDir = toString cfg.scratchDataDir;
  scratchImportDir = "${scratchDataDir}/imports";
  ocrGpuLeaseMarker = "${if dynamicStorageEnabled then scratchDataDir else toString cfg.dataDir}/ocr-gpu-lease-active";
  dynamicStorageServiceWritePaths = [
    scratchDataDir
    "-${preferredDataDir}"
  ];
  dynamicStoragePrepareWritePaths = [
    scratchDataDir
    "-${preferredDataParent}"
    "-${preferredDataDir}"
    runtimeStorageDir
  ];
  storageHelper = ''
    set -eu

    preferred=${lib.escapeShellArg preferredDataDir}
    preferred_mount=${lib.escapeShellArg preferredDataMountPoint}
    scratch=${lib.escapeShellArg scratchDataDir}
    user=${lib.escapeShellArg cfg.user}
    group=${lib.escapeShellArg cfg.group}
    runtime_dir=${lib.escapeShellArg runtimeStorageDir}
    env_file=${lib.escapeShellArg runtimeStorageEnvFile}
    scratch_marker="$scratch/.recitopia-scratch-active"

    write_env() {
      data_dir="$1"
      ${pkgs.coreutils}/bin/install -d -m 0755 "$runtime_dir"
      tmp="$env_file.tmp"
      {
        printf 'RECITOPIA_DB_PATH=%s/recitopia.duckdb\n' "$data_dir"
        printf 'RECITOPIA_IMPORT_DIR=%s/imports\n' "$data_dir"
      } > "$tmp"
      ${pkgs.coreutils}/bin/chmod 0644 "$tmp"
      ${pkgs.coreutils}/bin/mv "$tmp" "$env_file"
    }

    prepare_dir() {
      dir="$1"
      ${pkgs.coreutils}/bin/timeout 20 ${pkgs.coreutils}/bin/install -d -m 0750 -o "$user" -g "$group" "$dir" >/dev/null 2>&1
      ${pkgs.coreutils}/bin/timeout 20 ${pkgs.coreutils}/bin/install -d -m 0750 -o "$user" -g "$group" "$dir/imports" >/dev/null 2>&1
    }

    preferred_available() {
      [ -n "$preferred" ] || return 1
      if [ -n "$preferred_mount" ]; then
        ${pkgs.util-linux}/bin/mountpoint -q "$preferred_mount" || return 1
      fi
      prepare_dir "$preferred"
    }

    scratch_has_data() {
      [ -e "$scratch/recitopia.duckdb" ] && return 0
      [ -d "$scratch/imports" ] && [ -n "$(${pkgs.findutils}/bin/find "$scratch/imports" -mindepth 1 -print -quit 2>/dev/null)" ]
    }

    migrate_scratch_to_preferred() {
      prepare_dir "$scratch"
      prepare_dir "$preferred"
      if scratch_has_data && { [ -e "$scratch_marker" ] || [ ! -e "$preferred/recitopia.duckdb" ]; }; then
        if [ -e "$scratch/recitopia.duckdb" ]; then
          ${pkgs.rsync}/bin/rsync -a "$scratch/recitopia.duckdb" "$preferred/recitopia.duckdb"
        fi
        if [ -d "$scratch/imports" ]; then
          ${pkgs.rsync}/bin/rsync -a "$scratch/imports"/ "$preferred/imports"/
        fi
      fi
      ${pkgs.coreutils}/bin/rm -f "$preferred/.recitopia-scratch-active" "$scratch_marker"
      ${pkgs.coreutils}/bin/chown -R "$user:$group" "$preferred" >/dev/null 2>&1 || true
    }
  '';
  storagePrepareScript = pkgs.writeShellScript "recitopia-storage-prepare" ''
    ${storageHelper}

    prepare_dir "$scratch"
    if preferred_available; then
      migrate_scratch_to_preferred
      write_env "$preferred"
    else
      ${pkgs.coreutils}/bin/touch "$scratch_marker"
      ${pkgs.coreutils}/bin/chown "$user:$group" "$scratch_marker" >/dev/null 2>&1 || true
      write_env "$scratch"
    fi
  '';
  storageMigrateScript = pkgs.writeShellScript "recitopia-storage-migrate" ''
    ${storageHelper}

    [ -e "$scratch_marker" ] || exit 0
    preferred_available || exit 0

    api_was_active=0
    ocr_was_active=0
    if ${pkgs.systemd}/bin/systemctl is-active --quiet recitopia-api.service; then
      api_was_active=1
      ${pkgs.systemd}/bin/systemctl stop recitopia-api.service
    fi
    if ${pkgs.systemd}/bin/systemctl is-active --quiet recitopia-ocr.service; then
      ocr_was_active=1
      ${pkgs.systemd}/bin/systemctl stop recitopia-ocr.service
    fi

    migrate_scratch_to_preferred
    write_env "$preferred"

    if [ "$ocr_was_active" = 1 ]; then
      ${pkgs.systemd}/bin/systemctl start recitopia-ocr.service || true
    fi
    if [ "$api_was_active" = 1 ]; then
      ${pkgs.systemd}/bin/systemctl start recitopia-api.service
    fi
  '';
  ocrGpuAcquireScript = pkgs.writeShellScript "recitopia-ocr-gpu-acquire" ''
    set -eu
    marker=${lib.escapeShellArg ocrGpuLeaseMarker}
    unit=${lib.escapeShellArg ocrGpuPauseUnit}

    load_state="$(${pkgs.systemd}/bin/systemctl show --property=LoadState --value "$unit")"
    if [ "$load_state" != "loaded" ]; then
      echo "configured GPU pause unit is not loaded: $unit" >&2
      exit 1
    fi

    # An inactive peer plus a marker means another OCR batch already holds the
    # lease. If the peer is active, the marker survived a reboot/crash and is
    # stale; clear it before acquiring a fresh lease.
    if [ -e "$marker" ]; then
      if ${pkgs.systemd}/bin/systemctl is-active --quiet "$unit"; then
        ${pkgs.coreutils}/bin/rm -f "$marker"
      else
        exit 0
      fi
    fi

    if ${pkgs.systemd}/bin/systemctl is-active --quiet "$unit"; then
      ${pkgs.coreutils}/bin/touch "$marker"
      if ! ${pkgs.systemd}/bin/systemctl stop "$unit"; then
        ${pkgs.coreutils}/bin/rm -f "$marker"
        exit 1
      fi
    fi
  '';
  ocrGpuReleaseScript = pkgs.writeShellScript "recitopia-ocr-gpu-release" ''
    set -eu
    marker=${lib.escapeShellArg ocrGpuLeaseMarker}
    unit=${lib.escapeShellArg ocrGpuPauseUnit}

    # Only restart a service that the acquire helper actually stopped.
    if [ -e "$marker" ]; then
      ${pkgs.systemd}/bin/systemctl start "$unit"
      ${pkgs.coreutils}/bin/rm -f "$marker"
    fi
  '';
in
{
  options.services.recitopia-api = {
    enable = lib.mkEnableOption "Recitopia API";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ../apps/api-rs/package.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ../apps/api-rs/package.nix { }";
      description = "Recitopia API package to run.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "recitopia";
      description = "User that runs the API service.";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "recitopia";
      description = "Group that runs the API service.";
    };

    host = lib.mkOption {
      type = lib.types.str;
      default = "0.0.0.0";
      description = "Address for the API to bind. Use a Tailscale IP for the narrowest exposure.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 8077;
      description = "TCP port for the API.";
    };

    dataDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/recitopia";
      description = "Directory containing the DuckDB database.";
    };

    importDir = lib.mkOption {
      type = lib.types.path;
      default = "${cfg.dataDir}/imports";
      description = "Directory containing uploaded recipe import images, cookbook page originals, and mapper request files.";
    };

    preferredDataDir = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = "/mnt/nas/recitopia";
      description = ''
        Optional preferred persistent storage root. When set, Recitopia starts
        from this directory if it is writable, otherwise it falls back to
        scratchDataDir and a timer migrates scratch data back here when it
        becomes available.
      '';
    };

    preferredDataMountPoint = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = "/mnt/nas";
      description = ''
        Optional mountpoint that must be mounted before preferredDataDir is
        considered available. Set this for NAS-backed storage to avoid writing
        fallback data into an unmounted local mount directory.
      '';
    };

    scratchDataDir = lib.mkOption {
      type = lib.types.path;
      default = cfg.dataDir;
      defaultText = lib.literalExpression "config.services.recitopia-api.dataDir";
      example = "/var/lib/recitopia";
      description = "Local scratch storage root used when preferredDataDir is unavailable.";
    };

    storageMigrateInterval = lib.mkOption {
      type = lib.types.str;
      default = "5min";
      description = "How often to retry migrating scratch storage to preferredDataDir.";
    };

    environmentFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = "/etc/recitopia-api.env";
      description = "Optional legacy systemd EnvironmentFile for secrets such as RECITOPIA_LLM_API_KEY.";
    };

    environmentFiles = lib.mkOption {
      type = lib.types.listOf lib.types.path;
      default = [
        "/etc/recitopia-api.env"
        "/etc/recitopia/llm"
      ];
      example = [
        "/etc/recitopia/llm"
      ];
      description = "Optional systemd EnvironmentFile paths. Files use KEY=value syntax and may contain RECITOPIA_LLM_API_KEY.";
    };

    ocrPython = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = "/var/lib/recitopia/ocr-venv/bin/python";
      description = "Optional Python executable with PaddleOCR installed.";
    };

    ocrServerEnable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Run a local long-lived PaddleOCR service when ocrPython is configured.";
    };

    ocrServerHost = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      description = "Host address for the local PaddleOCR service.";
    };

    ocrServerPort = lib.mkOption {
      type = lib.types.port;
      default = 8078;
      description = "Port for the local PaddleOCR service.";
    };

    ocrGpuPauseUnit = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "voice-transcript.service";
      description = "Optional systemd service to stop while PaddleOCR owns the GPU, then restore if it was previously active.";
    };

    ocrGpuReleaseDelaySeconds = lib.mkOption {
      type = lib.types.ints.between 0 600;
      default = 20;
      description = "Seconds to retain the OCR GPU lease after a batch, avoiding stop/start churn between adjacent batches.";
    };

    ocrPrepareParallelism = lib.mkOption {
      type = lib.types.int;
      default = 2;
      description = "Number of concurrent OCR image preparation workers. This covers resize and crop before PaddleOCR runs.";
    };

    ocrImageMaxDimension = lib.mkOption {
      type = lib.types.int;
      default = 1600;
      description = "Maximum long edge for temporary OCR input images. Set to 0 to OCR original-size images.";
    };

    ocrImageQuality = lib.mkOption {
      type = lib.types.int;
      default = 85;
      description = "JPEG quality for temporary OCR input images.";
    };

    llmPython = lib.mkOption {
      type = lib.types.path;
      default = "${pkgs.python3}/bin/python3";
      description = "Python executable for the LLM mapper script.";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Open the API port in the NixOS firewall.";
    };

    firewallInterface = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = "tailscale0";
      description = "Interface to open when openFirewall is true. Set null to open globally.";
    };

    extraEnvironment = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = "Additional environment variables for the API service.";
    };
  };

  config = lib.mkIf cfg.enable {
    users.groups = lib.mkIf (cfg.group == "recitopia") {
      recitopia = { };
    };

    users.users = lib.mkIf (cfg.user == "recitopia") {
      recitopia = {
        isSystemUser = true;
        group = cfg.group;
        home = toString cfg.dataDir;
        createHome = true;
      };
    };

    security.polkit.enable = lib.mkIf ocrGpuLeaseEnabled true;
    security.polkit.extraConfig = lib.mkIf ocrGpuLeaseEnabled ''
      polkit.addRule(function(action, subject) {
        var unit = action.lookup("unit");
        var verb = action.lookup("verb");
        if (action.id == "org.freedesktop.systemd1.manage-units" &&
            subject.user == ${builtins.toJSON cfg.user} &&
            verb == "start" &&
            (unit == ${builtins.toJSON ocrGpuAcquireUnit} ||
             unit == ${builtins.toJSON ocrGpuReleaseUnit})) {
          return polkit.Result.YES;
        }
      });
    '';

    systemd.tmpfiles.rules = if dynamicStorageEnabled then [
      "d ${scratchDataDir} 0750 ${cfg.user} ${cfg.group} - -"
      "d ${scratchImportDir} 0750 ${cfg.user} ${cfg.group} - -"
    ] else [
      "d ${toString cfg.dataDir} 0750 ${cfg.user} ${cfg.group} - -"
      "d ${toString cfg.importDir} 0750 ${cfg.user} ${cfg.group} - -"
    ];

    networking.firewall = lib.mkIf cfg.openFirewall (
      if cfg.firewallInterface == null then
        { allowedTCPPorts = [ cfg.port ]; }
      else
        { interfaces = { "${cfg.firewallInterface}" = { allowedTCPPorts = [ cfg.port ]; }; }; }
    );

    systemd.services.recitopia-api = {
      description = "Recitopia API";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ]
        ++ lib.optional dynamicStorageEnabled "recitopia-storage-prepare.service"
        ++ lib.optional (cfg.ocrPython != null && cfg.ocrServerEnable) "recitopia-ocr.service";
      wants = [ "network-online.target" ]
        ++ lib.optional (cfg.ocrPython != null && cfg.ocrServerEnable) "recitopia-ocr.service";
      requires = lib.optional dynamicStorageEnabled "recitopia-storage-prepare.service";

      environment = {
        RECITOPIA_API_HOST = cfg.host;
        RECITOPIA_API_PORT = toString cfg.port;
        RECITOPIA_RUST_STORE_MODE = "read-write";
        RECITOPIA_OCR_SCRIPT = "${cfg.package}/share/recitopia/tools/ocr/paddle_ocr.py";
        RECITOPIA_LLM_PYTHON = toString cfg.llmPython;
        RECITOPIA_LLM_SCRIPT = "${cfg.package}/share/recitopia/tools/ml/llm_mapper.py";
        RECITOPIA_LLM_COOKBOOK_SCRIPT = "${cfg.package}/share/recitopia/tools/ml/llm_cookbook_mapper.py";
        RECITOPIA_TAR_BIN = "${pkgs.gnutar}/bin/tar";
        # AVIF page-image derivatives for the source-review UI; the API falls
        # back to serving originals if the converter is missing or fails.
        RECITOPIA_IMAGE_CONVERT_BIN = "${pkgs.imagemagick}/bin/magick";
        RECITOPIA_OCR_IMAGE_CONVERT_BIN = "${pkgs.imagemagick}/bin/magick";
        RECITOPIA_OCR_IMAGE_MAX_DIMENSION = toString cfg.ocrImageMaxDimension;
        RECITOPIA_OCR_IMAGE_QUALITY = toString cfg.ocrImageQuality;
        RECITOPIA_OCR_PREPARE_PARALLELISM = toString cfg.ocrPrepareParallelism;
      } // lib.optionalAttrs (!dynamicStorageEnabled) {
        RECITOPIA_DB_PATH = "${toString cfg.dataDir}/recitopia.duckdb";
        RECITOPIA_IMPORT_DIR = toString cfg.importDir;
      } // lib.optionalAttrs (cfg.ocrPython != null) {
        LD_LIBRARY_PATH = ocrLibraryPath;
        RECITOPIA_OCR_PYTHON = toString (pkgs.writeShellScript "recitopia-ocr-python" ''
          export LD_LIBRARY_PATH="${ocrLibraryPath}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
          exec ${toString cfg.ocrPython} "$@"
        '');
      } // lib.optionalAttrs (cfg.ocrPython != null && cfg.ocrServerEnable) {
        RECITOPIA_OCR_SERVER_URL = "http://${cfg.ocrServerHost}:${toString cfg.ocrServerPort}";
      } // cfg.extraEnvironment;

      serviceConfig = {
        Type = "simple";
        ExecStart = lib.getExe cfg.package;
        User = cfg.user;
        Group = cfg.group;
        WorkingDirectory = if dynamicStorageEnabled then runtimeStorageDir else toString cfg.dataDir;
        Restart = "on-failure";
        RestartSec = "5s";
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        ReadWritePaths = if dynamicStorageEnabled then dynamicStorageServiceWritePaths else [
          (toString cfg.dataDir)
          (toString cfg.importDir)
        ];
      } // lib.optionalAttrs (environmentFiles != [] || dynamicStorageEnabled) {
        EnvironmentFile = environmentFiles ++ lib.optional dynamicStorageEnabled runtimeStorageEnvFile;
      };
    };

    systemd.services.recitopia-storage-prepare = lib.mkIf dynamicStorageEnabled {
      description = "Prepare Recitopia storage and choose NAS or scratch";
      before = [ "recitopia-api.service" ]
        ++ lib.optional (cfg.ocrPython != null && cfg.ocrServerEnable) "recitopia-ocr.service";
      serviceConfig = {
        Type = "oneshot";
        ExecStart = storagePrepareScript;
        User = "root";
        Group = "root";
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        RuntimeDirectory = "recitopia";
        RuntimeDirectoryPreserve = true;
        ReadWritePaths = dynamicStoragePrepareWritePaths;
      };
    };

    systemd.services.recitopia-storage-migrate = lib.mkIf dynamicStorageEnabled {
      description = "Migrate Recitopia scratch storage to preferred storage";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = storageMigrateScript;
        User = "root";
        Group = "root";
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        RuntimeDirectory = "recitopia";
        RuntimeDirectoryPreserve = true;
        ReadWritePaths = dynamicStoragePrepareWritePaths;
      };
    };

    systemd.timers.recitopia-storage-migrate = lib.mkIf dynamicStorageEnabled {
      description = "Retry Recitopia scratch-to-preferred storage migration";
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "2min";
        OnUnitActiveSec = cfg.storageMigrateInterval;
        Unit = "recitopia-storage-migrate.service";
      };
    };

    systemd.services.recitopia-ocr-gpu-acquire = lib.mkIf ocrGpuLeaseEnabled {
      description = "Pause the configured GPU peer for Recitopia OCR";
      serviceConfig = {
        Type = "oneshot";
        ExecStart = ocrGpuAcquireScript;
        User = "root";
        Group = "root";
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        ReadWritePaths = if dynamicStorageEnabled then [ scratchDataDir ] else [ (toString cfg.dataDir) ];
      };
    };

    systemd.services.recitopia-ocr-gpu-release = lib.mkIf ocrGpuLeaseEnabled {
      description = "Restore the GPU peer paused by Recitopia OCR";
      serviceConfig = {
        Type = "oneshot";
        ExecStart = ocrGpuReleaseScript;
        User = "root";
        Group = "root";
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        ReadWritePaths = if dynamicStorageEnabled then [ scratchDataDir ] else [ (toString cfg.dataDir) ];
      };
    };

    systemd.services.recitopia-ocr = lib.mkIf (cfg.ocrPython != null && cfg.ocrServerEnable) {
      description = "Recitopia OCR service";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" "nvidia-persistenced.service" ]
        ++ lib.optional dynamicStorageEnabled "recitopia-storage-prepare.service";
      wants = [ "network-online.target" ];
      requires = lib.optional dynamicStorageEnabled "recitopia-storage-prepare.service";

      environment = {
        HOME = if dynamicStorageEnabled then scratchDataDir else toString cfg.dataDir;
        LD_LIBRARY_PATH = ocrLibraryPath;
        RECITOPIA_OCR_SERVER_HOST = cfg.ocrServerHost;
        RECITOPIA_OCR_SERVER_PORT = toString cfg.ocrServerPort;
      } // lib.optionalAttrs ocrGpuLeaseEnabled {
        RECITOPIA_OCR_GPU_ACQUIRE_UNIT = ocrGpuAcquireUnit;
        RECITOPIA_OCR_GPU_RELEASE_UNIT = ocrGpuReleaseUnit;
        RECITOPIA_OCR_GPU_RELEASE_DELAY_SECONDS = toString cfg.ocrGpuReleaseDelaySeconds;
        RECITOPIA_OCR_SYSTEMCTL_BIN = "${pkgs.systemd}/bin/systemctl";
      } // cfg.extraEnvironment;

      serviceConfig = {
        Type = "simple";
        ExecStart = "${toString cfg.ocrPython} ${cfg.package}/share/recitopia/tools/ocr/paddle_ocr_server.py";
        User = cfg.user;
        Group = cfg.group;
        WorkingDirectory = if dynamicStorageEnabled then scratchDataDir else toString cfg.dataDir;
        Restart = "on-failure";
        RestartSec = "5s";
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        ReadWritePaths = if dynamicStorageEnabled then dynamicStorageServiceWritePaths else [
          (toString cfg.dataDir)
          (toString cfg.importDir)
        ];
      } // lib.optionalAttrs ocrGpuLeaseEnabled {
        # A hard crash during a leased batch must not leave the peer stopped.
        ExecStopPost = "+${ocrGpuReleaseScript}";
      };
    };
  };
}
