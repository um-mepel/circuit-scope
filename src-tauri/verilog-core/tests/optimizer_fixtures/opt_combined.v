// Combined: constant propagation + strength reduction + fold.
// tmp = 2, y = tmp * 4  →  y = 2 << 2  →  y = 8
module opt_combined(output [7:0] y);
  wire [7:0] tmp;
  assign tmp = 2;
  assign y = tmp * 4;
endmodule
