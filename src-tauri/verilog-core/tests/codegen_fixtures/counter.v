// Simple 4-bit counter for VCD codegen testing (clock from `always #` — no synthetic driver).
module counter(output reg [3:0] count);
  reg clk;
  reg rst;
  initial begin
    clk = 0;
    rst = 0;
  end
  always #5 clk = ~clk;
  always @(posedge clk) begin
    if (rst)
      count <= 0;
    else
      count <= count + 1;
  end
endmodule
