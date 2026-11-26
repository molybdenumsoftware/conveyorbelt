final: prev: {
  crate2nix-tools = final.callPackage "${final.crate2nix.src}/tools.nix" {}; # IFD
  conveyorbeltCargoNix = final.callPackage ./pkgs/package-cargo-nix.nix {};
  conveyorbeltWorkspace = final.callPackage ./pkgs/workspace.nix {};
  conveyorbelt = final.callPackage ./pkgs/package.nix {};
}
