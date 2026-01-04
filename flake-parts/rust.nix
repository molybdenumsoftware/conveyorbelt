{
  perSystem =
    { pkgs, ... }:
    {
      gitignore = [ "/target" ];

      make-shells.default.packages = [
        pkgs.clippy
        pkgs.rust-analyzer
      ];
    };
}
