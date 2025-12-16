{ repx }:

repx.mkPipe rec {
  producer_a = repx.callStage ./stages/stage-A-producer.nix [ ];
  producer_b = repx.callStage ./stages/stage-B-producer.nix [ ];

  consumer = repx.callStage ./stages/stage-C-consumer.nix [
    producer_a
    [
      producer_b
      "raw_output"
      "list_b"
    ]
  ];

  partial_sums = repx.callStage ./stages/stage-D-scatter-sum.nix [
    [
      consumer
      "combined_list"
      "number_list_file"
    ]
  ];

  total_sum = repx.callStage ./stages/stage-E-transformer.nix [
    partial_sums
  ];
}
