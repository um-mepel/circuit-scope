// Constant folding: all RHS are compile-time computable.
module opt_const_fold(output [7:0] y1, output [7:0] y2, output y3, output y4);
  assign y1 = 3 + 5;
  assign y2 = (2 + 3) * 4;
  assign y3 = 10 > 3;
  assign y4 = 7 == 7;
endmodule
