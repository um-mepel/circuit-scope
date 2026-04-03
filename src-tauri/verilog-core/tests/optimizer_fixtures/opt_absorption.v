// Absorption: a & (a | b) = a, a | (a & b) = a.
module opt_absorption(input a, input b, output y1, output y2);
  assign y1 = a & (a | b);
  assign y2 = a | (a & b);
endmodule
