{
  config.perSystem = psArgs: {
    buildEnv = {
      CARGO_PROFILE = "dev";
      LOG_FILTER_VAR_NAME = "LOG";
      LOG = "info";
    };

    buildArgs.dontStrip = true;
  };
}
