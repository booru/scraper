flake: { config, lib, pkgs, ... }:

let
  inherit (lib) mkEnableOption mkOption types;

  inherit (flake.packages.${pkgs.stdenv.hostPlatform.system}) scraper-rs;

  cfg = config.services.scraper-rs;
in
{
  options = {
    services.scraper-rs = {
      enable = mkEnableOption ''
        General Media Page Scraper for Thumbnailing
      '';

      package = mkOption {
        type = types.package;
        default = flake.packages.${pkgs.stdenv.hostPlatform.system}.default;
        description = ''
          Scraper Package to use
        '';
      };

      port = mkOption {
        type = types.port;
        default = 8080;
        example = 9090;
        description = ''
          Port that Scraper will listen on
        '';
      };

      env-file = mkOption {
        type = types.nullOr types.path;
        default = null;
        description = ''
          Env file to source before starting Scraper
        '';
      };

      env = mkOption {
        type = types.attrs;
        default = {};
        description = ''
          Env Variables to set
        '';
      }
    };
  };

  config = lib.mkIf cfg.enable {

    systemd.services.scraper-rs = {
      description = "Scraper Media Thumbnailer";

      after = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];

      serviceConfig = {
        Restart = "on-failure";
        ExecStart = builtins.concatStringsSep " " [
          "${lib.getBin cfg.package}/bin/scraper"
          "--listen-on ${toString cfg.port}"
        ];
        Environment = cfg.env;
        EnvironmentFile = lib.mkIf (cfg.env-file != null) cfg.env-file;
        StateDirectory = "camoflage";
        StateDirectoryMode = "0750";

        CapabilityBoundingSet = [ "AF_NETLINK" "AF_INET" "AF_INET6" ];
        LockPersonality = true;
        NoNewPrivileges = true;
        PrivateDevices = true;
        PrivateTmp = true;
        PrivateUsers = true;
        ProtectClock = true;
        ProtectControlGroups = true;
        ProtectHome = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectSystem = "strict";
        ReadOnlyPaths = [ "/" ];
        RemoveIPC = true;
        RestrictAddressFamilies = [ "AF_NETLINK" "AF_INET" "AF_INET6" ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [ "@system-service" "~@privileged" "~@resources" "@pkey" ];
        UMask = "0027";
      };
    };
  };
}