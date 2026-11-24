inputs:
inputs.flake-parts.lib.mkFlake {inherit inputs;} {
  _module.args.rootPath = ./.;
  imports = [(inputs.import-tree ./nix/flake-parts)];
}
