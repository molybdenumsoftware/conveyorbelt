{
  lib,
  callPackage,
  conveyorbeltCargoNix,
}:
# IFD
callPackage conveyorbeltCargoNix {
  buildRustCrateForPkgs = pkgs:
    pkgs.buildRustCrate.override {
      defaultCrateOverrides =
        pkgs.defaultCrateOverrides
        // {
          static-web-server = attrs: {
            # otherwise retries forever
            env.CARGO_NET_OFFLINE = true;
          };

          conveyorbelt = attrs: let
            check.env = {
              NU_EXECUTABLE = lib.getExe pkgs.nushell;
              _BASH_EXECUTABLE = lib.getExe pkgs.bash;
              WESTON_EXECUTABLE = lib.getExe pkgs.westonLite;
              DBUS_DAEMON_EXECUTABLE = lib.getExe' pkgs.dbus "dbus-daemon";
              DBUS_SESSION_CONFIG_FILE = "${pkgs.dbus}/share/dbus-1/session.conf";
              GIT_BIN_PATH = "${pkgs.gitMinimal}/bin";
              CHROMIUM_BIN_PATH = "${pkgs.ungoogled-chromium}/bin";
            };
          in {
            env = check.env // {CARGO_CRATE_NAME = attrs.crateName;};
            dontStrip = true;
            extraRustcOpts = ["-Dwarnings" "-Wclippy::all"];
            passthru = {inherit check;};
            meta = import ./meta.nix lib;
          };
        };
    };
}
