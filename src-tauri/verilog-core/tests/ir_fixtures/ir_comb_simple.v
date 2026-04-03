// Simple combinational logic: tests expression parsing and operator precedence.
module ir_comb_simple(input a, input b, input c, output y);
  // y = a & b | ~c;
  assign y = (a & b) | ~c;
endmodule

