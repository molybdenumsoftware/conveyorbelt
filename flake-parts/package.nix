{
  lib,
  rootPath,
  config,
  ...
}:
{
  perSystem =
    psArgs@{ pkgs, ... }:
    {
      options.drv = lib.mkOption { type = lib.types.lazyAttrsOf lib.types.anything; };
      config = {
        drv.env = {
          SERVE_DIR = "serve";
          NU_EXECUTABLE = lib.getExe pkgs.nushell;
          XVFB_EXECUTABLE = lib.getExe' pkgs.xorg.xvfb "Xvfb";
          DBUS_DAEMON_EXECUTABLE = lib.getExe' pkgs.dbus "dbus-daemon";
          DBUS_SESSION_CONFIG_FILE = "${pkgs.dbus}/share/dbus-1/session.conf";
          GIT_BIN_PATH = "${pkgs.git}/bin";
          CHROMIUM_BIN_PATH = "${pkgs.chromium}/bin";
        };

        packages.default =
          psArgs.config.drv
          |> lib.recursiveUpdate {
            name = config.metadata.title;
            src = lib.fileset.toSource {
              root = rootPath;
              fileset = lib.fileset.unions [
                (rootPath + "/Cargo.lock")
                (rootPath + "/Cargo.toml")
                (rootPath + "/src")
                (rootPath + "/tests")
              ];
            };
            cargoLock.lockFile = rootPath + "/Cargo.lock";
          }
          |> pkgs.rustPlatform.buildRustPackage;

        make-shells.default = {
          inputsFrom = [ psArgs.config.packages.default ];
          inherit (psArgs.config.drv) env;
        };

        checks.package = psArgs.config.packages.default;
      };
    };
}
