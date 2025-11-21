{ repx }:

repx.mkPipe rec {
  # Stage 1: Generate a CSV of numbers (x, y)
  producer = repx.callStage ./stage-producer.nix [ ];

  # Stage 2: Calculate the sum of the 'y' column
  summer = repx.callStage ./stage-summer.nix [
    [
      producer
      "data.csv"
      "input_csv"
    ]
  ];
}
