// Simple three-module hierarchy used to test semantic analyzer.

module leaf(input a, output b);
  wire w;
  assign b = a;
endmodule

module mid(input a, output b);
  wire w;
  leaf u_leaf(.a(a), .b(b));
endmodule

module top(input x, output y);
  wire w;
  mid u_mid(.a(x), .b(y));
endmodule

