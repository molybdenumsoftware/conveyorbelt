{
  config.perSystem = psArgs: {
    buildEnv = {
      CARGO_PROFILE = "dev";
    };

    buildArgs.dontStrip = true;
  };
}
