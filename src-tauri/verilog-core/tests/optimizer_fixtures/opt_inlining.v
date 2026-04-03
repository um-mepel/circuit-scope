// Tests module inlining: small child should be inlined into parent.
module child_inv(input a, output y);
  wire t;
  assign t = ~a;
  assign y = t;
endmodule

module parent_inv(input x, output z);
  child_inv u0(.a(x), .y(z));
endmodule
