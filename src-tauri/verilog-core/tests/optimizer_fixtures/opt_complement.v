// Complement rules: x & ~x = 0, x | ~x = all 1s.
module opt_complement(input [7:0] a, output [7:0] y1, output [7:0] y2);
  assign y1 = a & ~a;
  assign y2 = a | ~a;
endmodule
