{
  pkgs,
  repx,
  simple-lab,
  sweep-lab,
}:

let
  test = pkgs.testers.runNixOSTest {
    name = "repx-doc-assets-generation";

    nodes.machine =
      { pkgs, ... }:
      {
        imports = [ ];

        environment.systemPackages = [
          repx
          pkgs.vhs
          pkgs.chromium
          pkgs.fontconfig
          pkgs.glibcLocales
          pkgs.graphviz
        ];

        users.users.docs = {
          isNormalUser = true;
          uid = 1000;
          extraGroups = [ "wheel" ];
        };

        fonts.packages = [
          pkgs.nerd-fonts.jetbrains-mono
        ];

        fonts.fontconfig.enable = true;
      };

    testScript = ''
      import os

      out_dir = os.environ.get('out')

      start_all()

      machine.succeed("mkdir -p /tmp/assets")
      machine.succeed("chown docs:users /tmp/assets")

      print("Checking environment...")
      machine.succeed("su - docs -c 'which dot'")
      machine.succeed("su - docs -c 'which repx'")

      print("Generating SVGs...")
      machine.succeed("su - docs -c 'repx viz --lab ${simple-lab} --output /tmp/assets/simple-topology --format svg'")
      machine.succeed("su - docs -c 'repx viz --lab ${sweep-lab} --output /tmp/assets/parameter-sweep-topology --format svg'")

      print("Preparing VHS tape...")
      tape_content = """
      Output "/tmp/assets/simple-tui.gif"
      Set FontFamily "JetBrainsMono Nerd Font"
      Set FontSize 16
      Set Width 1200
      Set Height 800
      Set Padding 10
      Set Theme "Dracula"

      Hide
      Type "repx tui --lab ${simple-lab}"
      Enter
      Sleep 10s
      Show

      Sleep 2s
      Screenshot "/tmp/assets/simple-tui.png"
      """

      machine.succeed(f"su - docs -c 'cat <<EOF > /tmp/assets/script.tape\n{tape_content}\nEOF'")

      print("Running VHS inside VM...")
      machine.succeed("su - docs -c 'export XDG_CACHE_HOME=/tmp/assets/.cache; vhs /tmp/assets/script.tape'")

      print("Extracting assets...")
      machine.copy_from_vm("/tmp/assets/simple-topology.svg", f"{out_dir}/simple-topology.svg")
      machine.copy_from_vm("/tmp/assets/parameter-sweep-topology.svg", f"{out_dir}/parameter-sweep-topology.svg")
      machine.copy_from_vm("/tmp/assets/simple-tui.gif", f"{out_dir}/simple-tui.gif")
      machine.copy_from_vm("/tmp/assets/simple-tui.png", f"{out_dir}/simple-tui.png")
    '';
  };
in
test
