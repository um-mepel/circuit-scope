// Regression: `.hex(HEX6[6:0])` must carry the same time-varying values as `.hex(HEX7)`.
// A constant-driven slice can mask an undriven-port bug (both stuck at one literal).
module seg7_drv(output [6:0] hex, input [2:0] digit);
  assign hex = (digit == 3'd0) ? 7'h30 : 7'h7F;
endmodule

module instance_output_partselect_dynamic(
  output [6:0] HEX7,
  output [6:0] HEX6
);
  wire [2:0] digit;
  reg [2:0] d;
  reg clk;
  initial begin
    clk = 0;
    d = 3'd0;
  end
  always #5 clk = ~clk;
  always @(posedge clk) d <= d + 3'd1;
  assign digit = d;

  seg7_drv u7(.hex(HEX7), .digit(digit));
  seg7_drv u6(.hex(HEX6[6:0]), .digit(digit));
endmodule
