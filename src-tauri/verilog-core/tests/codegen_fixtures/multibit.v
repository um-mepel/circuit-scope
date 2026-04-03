module multibit(
    input [7:0] data_in,
    output reg [7:0] data_out,
    output [3:0] nibble
);

assign nibble = data_in[3:0];

reg clk;
initial clk = 0;
always #5 clk = ~clk;

always @(posedge clk) begin
    data_out <= data_in;
end

endmodule
