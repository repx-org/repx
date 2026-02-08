{ repx }:

repx.mkPipe rec {
  producer = repx.callStage ./stage-producer.nix [ ];

  summer = repx.callStage ./stage-summer.nix [
    [
      producer
      "data.csv"
      "input_csv"
    ]
  ];
}
