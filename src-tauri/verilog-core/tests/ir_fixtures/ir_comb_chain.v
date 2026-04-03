// Combinational chain: tests multiple binary ops and left-associativity.
module ir_comb_chain(input [3:0] a, input [3:0] b, input [3:0] c, output [3:0] y);
  // y = a + b * c - 1;
  assign y = a + b * c - 4'd1;
endmodule

