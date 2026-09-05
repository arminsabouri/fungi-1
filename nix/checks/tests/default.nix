{
  perSystem =
    {
      cargoWorkspaces,
      pkgs,
      toolchains,
      ...
    }:
    let
      profiles = {
        dev = "dev";
        release = "release";
      };
      workspace = import ../workspace.nix { inherit cargoWorkspaces pkgs; };
      mkTest =
        cargoWorkspace: profile: craneLib:
        let
          deps = craneLib.buildDepsOnly (cargoWorkspace.commonArgs // { CARGO_PROFILE = profile; });
        in
        craneLib.cargoNextest (
          (workspace.checkArgs cargoWorkspace)
          // {
            cargoArtifacts = deps;
            CARGO_PROFILE = profile;
            cargoNextestExtraArgs = "--user-config-file ${./nextest-record.toml}";
            nativeBuildInputs = [ pkgs.unzip ];
            preCheck = ''
              export NEXTEST_STATE_DIR="$TMPDIR/nextest-state"
              mkdir -p "$NEXTEST_STATE_DIR"
            '';
            postCheck = ''
              cargo nextest store export \
                --no-pager \
                --user-config-file ${./nextest-record.toml} \
                --archive-file "$out/nextest-run.zip" \
                latest
              unzip -tqq "$out/nextest-run.zip"
            '';
          }
        );
    in
    {
      checks = pkgs.lib.concatMapAttrs (
        workspaceName: cargoWorkspace:
        pkgs.lib.concatMapAttrs (
          toolchainName: craneLib:
          pkgs.lib.mapAttrs' (
            profileName: profile:
            pkgs.lib.nameValuePair "${workspaceName}-tests-${toolchainName}-${profileName}" (
              mkTest cargoWorkspace profile craneLib
            )
          ) profiles
        ) toolchains
      ) cargoWorkspaces;
    };
}
