// Ternary simplification: constant condition, same arms.
module opt_ternary(input a, input b, input sel, output y1, output y2);
  assign y1 = 1 ? a : b;
  assign y2 = sel ? a : a;
endmodule
