{
  lib,
  crate2nix-tools,
}: let
  root = ../..;
in
  crate2nix-tools.generatedCargoNix {
    name = "conveyorbelt";
    src = lib.fileset.toSource {
      inherit root;
      fileset = lib.fileset.unions [
        (lib.fileset.fileFilter (file: lib.elem file.name ["Cargo.toml" "Cargo.lock"]) root)
        (lib.fileset.fileFilter (file: file.hasExt "rs") root)
      ];
    };
  }
