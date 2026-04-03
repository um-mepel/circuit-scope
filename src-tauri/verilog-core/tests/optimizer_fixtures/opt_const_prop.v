// Constant propagation: tmp is a constant, gets propagated and folded.
module opt_const_prop(output [7:0] y);
  wire [7:0] tmp;
  assign tmp = 10;
  assign y = tmp + 5;
endmodule
