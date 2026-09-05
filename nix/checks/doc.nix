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
      checks = workspace.mapWorkspaces "doc" (
        _: cargoWorkspace:
        toolchains.nightly.cargoDoc (
          cargoWorkspace.commonArgs
          // {
            cargoArtifacts = cargoWorkspace.cargoArtifactsDev;
            CARGO_PROFILE = "dev";
            cargoDocExtraArgs = "--no-deps --all-features";
            RUSTDOCFLAGS = "-D warnings";
          }
        )
      );
    };
}
