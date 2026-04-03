// Copy propagation: t = a + 1, y = t + 2 → y = a + 3.
module opt_copy_prop(input [7:0] a, output [7:0] y);
  wire [7:0] t;
  assign t = a + 1;
  assign y = t + 2;
endmodule
