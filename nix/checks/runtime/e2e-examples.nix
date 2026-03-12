{
  pkgs,
  repx,
  repx-lib,
}:

let
  simpleLab =
    (import ../../../examples/simple/nix/lab.nix {
      inherit pkgs repx-lib;
      gitHash = "e2e-check";
    }).lab;

  paramSweepLab =
    (import ../../../examples/param-sweep/nix/lab.nix {
      inherit pkgs repx-lib;
      gitHash = "e2e-check";
    }).lab;

  pkgsImpureIncremental =
    let
      overlaySet = import ../../../examples/impure-incremental/nix/overlay.nix;
    in
    pkgs.extend (
      pkgs.lib.composeManyExtensions [
        overlaySet.common
        overlaySet.pure
      ]
    );

  impureIncrementalLab =
    (import ../../../examples/impure-incremental/nix/lab.nix {
      pkgs = pkgsImpureIncremental;
      inherit repx-lib;
      gitHash = "e2e-check";
    }).lab;

in
pkgs.testers.runNixOSTest {
  name = "e2e-examples";

  nodes.machine = _: {
    virtualisation = {
      diskSize = 25600;
      memorySize = 4096;
      cores = 2;
    };
    environment.systemPackages = [
      repx
      pkgs.bubblewrap
    ];
  };

  testScript = ''
    start_all()

    base_path = "/var/lib/repx-store"
    machine.succeed(f"mkdir -p {base_path}")

    machine.succeed("mkdir -p /root/.config/repx")

    config = f"""
    submission_target = "local"
    [targets.local]
    base_path = "{base_path}"
    default_scheduler = "local"
    default_execution_type = "bwrap"
    [targets.local.local]
    execution_types = ["bwrap"]
    local_concurrency = 4
    """
    machine.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    machine.succeed("mkdir -p /var/lib/repx-store/artifacts/host-tools/default/bin")
    machine.succeed("ln -s $(which bwrap) /var/lib/repx-store/artifacts/host-tools/default/bin/bwrap")

    print("\n" + "=" * 60)
    print("RUNNING EXAMPLE: simple")
    print("=" * 60)

    machine.succeed("repx run simulation-run analysis-run --lab ${simpleLab}")

    simple_success = int(machine.succeed(f"find {base_path}/outputs -name SUCCESS | wc -l").strip())
    print(f"simple: {simple_success} SUCCESS markers")
    if simple_success == 0:
        raise Exception("simple example: no SUCCESS markers found")

    machine.succeed(f"find {base_path}/outputs -name 'plot.png' | grep -q .")

    print("\n" + "=" * 60)
    print("RUNNING EXAMPLE: param-sweep")
    print("=" * 60)

    machine.succeed("repx run sweep-run plot-run --lab ${paramSweepLab}")

    sweep_success = int(machine.succeed(f"find {base_path}/outputs -name SUCCESS | wc -l").strip())
    print(f"param-sweep: {sweep_success} SUCCESS markers (cumulative)")
    if sweep_success <= simple_success:
        raise Exception("param-sweep example: no new SUCCESS markers found")

    machine.succeed(f"find {base_path}/outputs -name 'combined_plot.png' | grep -q .")

    print("\n" + "=" * 60)
    print("RUNNING EXAMPLE: impure-incremental (pure)")
    print("=" * 60)

    machine.succeed("repx run build --lab ${impureIncrementalLab}")

    total_success = int(machine.succeed(f"find {base_path}/outputs -name SUCCESS | wc -l").strip())
    print(f"impure-incremental: {total_success} SUCCESS markers (cumulative)")
    if total_success <= sweep_success:
        raise Exception("impure-incremental example: no new SUCCESS markers found")

    print("\n" + "=" * 60)
    print("ALL EXAMPLE E2E TESTS PASSED")
    print("=" * 60)
  '';
}
