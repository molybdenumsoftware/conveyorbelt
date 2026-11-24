{
  inputs,
  lib,
  ...
}: {
  flake-file.inputs.files = {
    url = "github:mightyiam/files";
    flake = false;
  };
  imports = ["${inputs.files}/flake-module.nix"];
  perSystem = psArgs: {
    files.writer.app = true;
    treefmt.settings.global.excludes = lib.attrNames psArgs.config.files.file;
  };
}
