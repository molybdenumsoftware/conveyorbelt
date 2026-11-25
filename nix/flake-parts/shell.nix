{inputs, ...}: {
  flake-file.inputs.make-shell = {
    url = "github:nicknovitski/make-shell";
    flake = false;
  };

  imports = ["${inputs.make-shell}/flake-module.nix"];

  perSystem = {pkgs, ...}: {
    make-shells.default = {
      shellHook = ''
        export SHELL="${pkgs.bashInteractive}/bin/bash"
      '';

      env =
        pkgs.conveyorbelt.passthru.check.env
        // {
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };

      packages = with pkgs; [
        cargo
        clippy
        rust-analyzer
        rustc
        rustfmt
      ];
    };
  };
}
