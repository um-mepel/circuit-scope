// Hierarchy for IR: top instantiates mid, which instantiates leaf.

module ir_leaf(input a, input b, output y);
  assign y = a ^ b;
endmodule

module ir_mid(input a, input b, output y);
  wire t;
  ir_leaf u0(.a(a), .b(b), .y(t));
  ir_leaf u1(.a(t), .b(b), .y(y));
endmodule

module ir_top(input [7:0] x, input [7:0] z, output [7:0] y);
  ir_mid u_mid(.a(x[0]), .b(z[0]), .y(y[0]));
endmodule

