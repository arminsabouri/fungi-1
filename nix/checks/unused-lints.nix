{
  perSystem =
    {
      cargoWorkspaces,
      pkgs,
      toolchains,
      ...
    }:
    let
      workspace = import ./workspace.nix { inherit cargoWorkspaces pkgs; };
    in
    {
      checks = workspace.mapWorkspaces "unused-lints" (
        _: cargoWorkspace:
        toolchains.nightly.mkCargoDerivation (
          cargoWorkspace.commonArgs
          // {
            cargoArtifacts = cargoWorkspace.cargoArtifactsDev;
            CARGO_PROFILE = "dev";
            pnameSuffix = "-unused-lints";
            buildPhaseCargoCommand = ''
              RUSTFLAGS="''${RUSTFLAGS:-} -D unused" \
                cargo check ${cargoWorkspace.commonArgs.cargoExtraArgs} --all-targets --all-features
            '';
            installPhase = "mkdir -p $out";
          }
        )
      );
    };
}
