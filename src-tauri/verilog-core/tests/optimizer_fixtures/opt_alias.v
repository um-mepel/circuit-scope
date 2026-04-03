// Wire alias elimination: b is an alias for a, c is an alias for b (chain).
module opt_alias(input [7:0] a, output [7:0] y);
  wire [7:0] b, c;
  assign b = a;
  assign c = b;
  assign y = c + 1;
endmodule
