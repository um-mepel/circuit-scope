// Regression: full-range output slice `.hex(HEX6[6:0])` must flatten to an assign like `.hex(HEX7)`.
module seg7(output [6:0] hex);
  assign hex = 7'h7F;
endmodule

module instance_output_partselect(
  output [6:0] HEX7,
  output [6:0] HEX6
);
  seg7 u7(.hex(HEX7));
  seg7 u6(.hex(HEX6[6:0]));
endmodule
