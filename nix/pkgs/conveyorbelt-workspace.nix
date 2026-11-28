{
  lib,
  callPackage,
  conveyorbeltCargoNix,
  conveyorbeltShared,
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

          test-environment = attrs: {
            env = {
              inherit
                (conveyorbeltShared.check.env)
                DBUS_DAEMON_EXECUTABLE
                DBUS_SESSION_CONFIG_FILE
                WESTON_EXECUTABLE
                ;
            };
          };

          conveyorbelt = attrs: {
            env = {
              CARGO_CRATE_NAME = attrs.crateName;
              inherit
                (conveyorbeltShared.check.env)
                CHROMIUM_BIN_PATH
                GIT_BIN_PATH
                LN_EXECUTABLE
                NU_EXECUTABLE
                _BASH_EXECUTABLE
                ;
            };
            dontStrip = true;
            extraRustcOpts = ["-Dwarnings" "-Wclippy::all"];
            meta = import ../meta.nix lib;
          };
        };
    };
}
