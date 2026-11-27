final: prev: {
  crate2nix-tools = final.callPackage "${final.crate2nix.src}/tools.nix" {}; # IFD
  conveyorbeltShared = final.callPackage ./pkgs/conveyorbelt-shared.nix {};
  conveyorbeltCargoNix = final.callPackage ./pkgs/conveyorbelt-cargo-nix.nix {};
  conveyorbeltWorkspace = final.callPackage ./pkgs/conveyorbelt-workspace.nix {};
  conveyorbelt = final.callPackage ./pkgs/conveyorbelt.nix {};
}
