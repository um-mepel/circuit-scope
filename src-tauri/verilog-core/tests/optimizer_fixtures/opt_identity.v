// Identity and annihilator rules.
module opt_identity(input [7:0] a, output [7:0] y1, output [7:0] y2,
                    output [7:0] y3, output [7:0] y4, output [7:0] y5);
  assign y1 = a + 0;
  assign y2 = a * 1;
  assign y3 = a & 0;
  assign y4 = a | 0;
  assign y5 = a ^ 0;
endmodule
