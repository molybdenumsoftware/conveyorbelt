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

          conveyorbelt = attrs: {
            env = conveyorbeltShared.check.env // {CARGO_CRATE_NAME = attrs.crateName;};
            dontStrip = true;
            extraRustcOpts = ["-Dwarnings" "-Wclippy::all"];
            meta = import ../meta.nix lib;
          };
        };
    };
}
