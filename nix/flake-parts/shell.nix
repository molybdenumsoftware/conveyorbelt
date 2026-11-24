{inputs, ...}: {
  flake-file.inputs.make-shell = {
    url = "github:nicknovitski/make-shell";
    flake = false;
  };

  imports = ["${inputs.make-shell}/flake-module.nix"];

  perSystem = {pkgs, ...}: {
    make-shells.default = {
      inputsFrom = [pkgs.conveyorbelt];

      env =
        pkgs.conveyorbelt.passthru.check.env
        // {
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };

      packages = [
        pkgs.rust-analyzer
        pkgs.rustfmt
      ];
    };
  };
}
