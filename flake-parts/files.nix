{ inputs, ... }:
{
  flake-file.inputs.files = {
    url = "github:mightyiam/files";
    flake = false;
  };
  imports = [ "${inputs.files}/flake-module.nix" ];
  perSystem = {
    files.writer.app = true;
  };
}
