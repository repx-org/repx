{
  pkgs,
  repx,
  simple-lab,
  sweep-lab,
}:

let
  termframe = pkgs.callPackage ../termframe.nix { };
  nerdFont = pkgs.nerd-fonts.jetbrains-mono;
  fontDir = "${nerdFont}/share/fonts/truetype/NerdFonts/JetBrainsMono";

  termframeConfig = pkgs.writeText "termframe-config.toml" ''
    [[fonts]]
    family = "JetBrains Mono"
    files = [
      "${fontDir}/JetBrainsMonoNerdFont-Regular.ttf",
      "${fontDir}/JetBrainsMonoNerdFont-Bold.ttf",
      "${fontDir}/JetBrainsMonoNerdFont-Italic.ttf",
      "${fontDir}/JetBrainsMonoNerdFont-BoldItalic.ttf",
    ]
  '';
in
pkgs.runCommand "repx-doc-assets"
  {
    nativeBuildInputs = [
      repx
      pkgs.graphviz
      termframe
    ];
  }
  ''
    mkdir -p $out

    export HOME=$(mktemp -d)

    echo "Generating topology SVGs..."
    repx viz --lab ${simple-lab} --output $out/simple-topology --format svg
    repx viz --lab ${sweep-lab} --output $out/parameter-sweep-topology --format svg

    echo "Generating TUI screenshot..."
    repx tui --lab ${simple-lab} --screenshot /tmp/tui-ansi.txt
    cat /tmp/tui-ansi.txt | termframe \
      --config - \
      --config ${termframeConfig} \
      --theme dracula \
      --font-family "JetBrainsMono Nerd Font" \
      --font-size 11 \
      --line-height 1.2 \
      --width 120 \
      --height 36 \
      --mode dark \
      --embed-fonts \
      --subset-fonts \
      --window \
      --window-shadow \
      --output $out/simple-tui.svg
  ''
