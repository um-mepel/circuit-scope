// Self-operations: x & x = x, x | x = x, x ^ x = 0, x - x = 0.
module opt_bitwise(input [7:0] a, output [7:0] y1, output [7:0] y2,
                   output [7:0] y3, output [7:0] y4);
  assign y1 = a & a;
  assign y2 = a | a;
  assign y3 = a ^ a;
  assign y4 = a - a;
endmodule
