final: prev: {
  crate2nix-tools = final.callPackage "${final.crate2nix.src}/tools.nix" {}; # IFD
  conveyorbeltCargoNix = final.callPackage ./package-cargo-nix.nix {};
  conveyorbeltWorkspace = final.callPackage ./workspace.nix {};
  conveyorbelt = final.callPackage ./package.nix {};
}
