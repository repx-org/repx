{
  lib,
  rustPlatform,
  fetchFromGitHub,
  installShellFiles,
}:

rustPlatform.buildRustPackage {
  pname = "termframe";
  version = "0.8.1";

  src = fetchFromGitHub {
    owner = "pamburus";
    repo = "termframe";
    rev = "v0.8.1";
    hash = "sha256-rW+45Idx2cehFOLxo6KJwVYLEuxlb+olZEQU7mn0HZg=";
  };

  cargoHash = "sha256-J8ceIWwhSb0pyiccVAGxJIcAkMNZS72uqUa8PU+ttRM=";

  patches = [ ./patches/termframe-local-fonts.patch ];

  nativeBuildInputs = [ installShellFiles ];

  postInstall = ''
    installShellCompletion --cmd termframe \
      --bash <($out/bin/termframe --shell-completions bash) \
      --fish <($out/bin/termframe --shell-completions fish) \
      --zsh <($out/bin/termframe --shell-completions zsh)
  '';

  doCheck = false;

  meta = {
    description = "Terminal output SVG screenshot tool";
    homepage = "https://github.com/pamburus/termframe";
    license = lib.licenses.mit;
    mainProgram = "termframe";
  };
}
