{
  config,
  lib,
  toSource,
  ...
}:
{
  cargoManifest.package.version = "0.0.0";

  perSystem =
    psArgs@{
      pkgs,
      craneLib,
      ...
    }:
    {
      checkEnv = {
        NU_EXECUTABLE = lib.getExe pkgs.nushell;
        XVFB_EXECUTABLE = lib.getExe' pkgs.xvfb "Xvfb";
        DBUS_DAEMON_EXECUTABLE = lib.getExe' pkgs.dbus "dbus-daemon";
        DBUS_SESSION_CONFIG_FILE = "${pkgs.dbus}/share/dbus-1/session.conf";
        GIT_BIN_PATH = "${pkgs.git}/bin";
        CHROMIUM_BIN_PATH = "${pkgs.chromium}/bin";
      };

      packages.default =
        psArgs.config.buildArgs
        // {
          inherit (psArgs.config.checks) cargoArtifacts;
          env = lib.mergeAttrsList [
            psArgs.config.buildEnv
            psArgs.config.checkEnv
            { HOME = "/build"; }
          ];

          src =
            [
              config.filesets.manifest
              config.filesets.lockFile
              config.filesets.sourceFiles
            ]
            |> lib.fileset.unions
            |> toSource;
        }
        |> craneLib.buildPackage;

      make-shells.default = {
        inputsFrom = [
          psArgs.config.packages.default
          psArgs.config.checks.clippy
        ];
        env = psArgs.config.buildEnv // psArgs.config.checkEnv;
      };

      checks = {
        cargoArtifacts =
          psArgs.config.buildArgs
          // {
            env = psArgs.config.buildEnv;
            src =
              [
                config.filesets.manifest
                config.filesets.lockFile
              ]
              |> lib.fileset.unions
              |> toSource;
          }
          |> craneLib.buildDepsOnly;

        package = psArgs.config.packages.default;

        clippy = craneLib.cargoClippy {
          inherit (psArgs.config.checks) cargoArtifacts;
          cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          env = psArgs.config.buildEnv // psArgs.config.checkEnv;

          src =
            [
              config.filesets.manifest
              config.filesets.lockFile
              config.filesets.sourceFiles
            ]
            |> lib.fileset.unions
            |> toSource;
        };
      };
    };
}
