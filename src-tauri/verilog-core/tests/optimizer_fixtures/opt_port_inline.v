// Tests port-mapped module inlining.
module adder(input a, input b, output y);
  assign y = a + b;
endmodule

module top_port_inline(input x1, input x2, output z);
  adder u_add(.a(x1), .b(x2), .y(z));
endmodule
