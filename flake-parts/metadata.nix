{ lib, config, ... }:
{
  options.metadata = {
    title = lib.mkOption {
      type = lib.types.singleLineStr;
      default = "conveyorbelt";
    };
    description = lib.mkOption {
      type = lib.types.singleLineStr;
      default = "A based web dev workflow; stack-agnostic, hand-coded, 🦀";
    };
  };
  config = {
    flake-file.description = config.metadata.description;

    binManifest.package = {
      description = config.metadata.description;
      name = config.metadata.title;
    };
  };
}
