// Tests VCD generation across a module hierarchy.
module inverter(input a, output y);
  assign y = ~a;
endmodule

module top_hier(input x, output z);
  inverter u0(.a(x), .y(z));
endmodule
