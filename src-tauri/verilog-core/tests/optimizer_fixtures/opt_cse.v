// CSE: two assignments compute the same expression.
module opt_cse(input [7:0] a, input [7:0] b, output [7:0] y);
  wire [7:0] t1, t2;
  assign t1 = a + b;
  assign t2 = a + b;
  assign y = t1 & t2;
endmodule
