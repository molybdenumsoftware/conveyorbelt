{
  config,
  lib,
  toSource,
  ...
}:
{
  perSystem =
    psArgs@{
      pkgs,
      craneLib,
      ...
    }:
    {
      checkEnv = {
        NU_EXECUTABLE = lib.getExe pkgs.nushell;
        _BASH_EXECUTABLE = lib.getExe pkgs.bash;
        FOO_EXECUTABLE = lib.getExe pkgs.bash;
        XVFB_EXECUTABLE = lib.getExe' pkgs.xorg-server "Xvfb";
        DBUS_DAEMON_EXECUTABLE = lib.getExe' pkgs.dbus "dbus-daemon";
        DBUS_SESSION_CONFIG_FILE = "${pkgs.dbus}/share/dbus-1/session.conf";
        GIT_BIN_PATH = "${pkgs.git}/bin";
        CHROMIUM_BIN_PATH = "${pkgs.ungoogled-chromium}/bin";
      };

      buildArgs = {
        pname = config.metadata.title;
        version = "git";
      };

      packages.default =
        psArgs.config.buildArgs
        // {
          inherit (psArgs.config.checks) cargoArtifacts;
          cargoExtraArgs = "--bin ${config.metadata.title}";
          env = lib.mergeAttrsList [
            psArgs.config.buildEnv
            psArgs.config.checkEnv
            { HOME = "/build"; }
          ];

          src =
            [
              config.filesets.workspaceManifest
              config.filesets.binManifest
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
                config.filesets.workspaceManifest
                config.filesets.binManifest
                config.filesets.lockFile
              ]
              |> lib.fileset.unions
              |> toSource;
          }
          |> craneLib.buildDepsOnly;

        package = psArgs.config.packages.default;

        clippy = craneLib.cargoClippy {
          inherit (psArgs.config.checks) cargoArtifacts;
          pname = "clippy";
          version = "git";
          cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          env = psArgs.config.buildEnv // psArgs.config.checkEnv;

          src =
            [
              config.filesets.workspaceManifest
              config.filesets.binManifest
              config.filesets.lockFile
              config.filesets.sourceFiles
            ]
            |> lib.fileset.unions
            |> toSource;
        };
      };
    };
}
