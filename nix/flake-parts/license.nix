{inputs, ...}: {
  flake-file.inputs.license = {
    url = "https://spdx.org/licenses/MIT.txt";
    flake = false;
  };

  perSystem = {
    files.file.LICENSE.text = builtins.readFile inputs.license;
  };
}
