// Strength reduction: mul/div/mod by powers of 2.
module opt_strength(input [7:0] a, output [7:0] y1, output [7:0] y2, output [7:0] y3);
  assign y1 = a * 8;
  assign y2 = a / 4;
  assign y3 = a % 16;
endmodule
