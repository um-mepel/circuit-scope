`timescale 1s/100ms
// EECS 270 Project 6 — TestBench6 fixture (no Clock_Div; direct CLK like coursework sim).
module TestBench6();
	reg CLK;
	reg [17:0] SW;
	wire [6:0] HEX7, HEX6;
	wire [6:0] HEX3;
	wire [6:0] HEX2;
	wire [6:0] HEX1;
	wire [6:0] HEX0;

TLC controller(.Clock(CLK), .Reset(SW[17]), .E(SW[3]), .NL(SW[2]), .EL(SW[1]), .W(SW[0]),
.TimerMSD(HEX7), .TimerLSD(HEX6), .ETL(HEX3), .NLTL(HEX2), .ELTL(HEX1), .WTL(HEX0));

wire s17, s3, s2, s1, s0;
assign s17 = SW[17];
assign s3 = SW[3];
assign s2 = SW[2];
assign s1 = SW[1];
assign s0 = SW[0];

initial CLK = 1;

always #0.5 CLK = ~CLK;

	initial
	begin
		SW = 0;
		SW[3] = 1;
		#33;
		SW = 0;
		SW[17] = 1;
		#11;
		SW[17] = 0;
		#22;
		SW[3] = 1;
		#22;
		SW[3] = 0;
		#22;
		SW[2] = 1;
		#22;
		SW[2] = 0;
		#22;
		SW[1] = 1;
		#22;
		SW[1] = 0;
		#22;
		SW[0] = 1;
		#22;
		SW[0] = 0;
		#22;
		SW[3] = 1;
		SW[2] = 1;
		#22;
		SW[3] = 0;
		SW[2] = 0;
		#22;
		SW[1] = 1;
		SW[0] = 1;
		#22;
		SW[1] = 0;
		SW[0] = 0;
		#22;
		SW[3] = 1;
		SW[1] = 1;
		#22;
		SW[3] = 0;
		SW[1] = 0;
		#22;
		SW[2] = 1;
		SW[0] = 1;
		#22;
		SW[2] = 0;
		SW[0] = 0;
		#44;
		SW[3] = 1;
		SW[2] = 1;
		SW[1] = 1;
		SW[0] = 1;
		#22;
		SW[17] = 1;
		#4;
		SW[17] = 0;
		SW = 0;
		#66;
		SW[3] = 1;
		SW[2] = 1;
		SW[1] = 1;
		#22;
		SW[3] = 0;
		SW[2] = 0;
		SW[1] = 0;
		#22;
		SW[3] = 1;
		SW[2] = 1;
		SW[0] = 1;
		#22;
		SW[3] = 0;
		SW[2] = 0;
		SW[0] = 0;
		#22;
		SW[3] = 1;
		SW[0] = 1;
		#22;
		SW[3] = 0;
		SW[0] = 0;
		#22;
		SW[2] = 1;
		SW[1] = 1;
		#22;
		SW[2] = 0;
		SW[1] = 0;
		#22;
	end
endmodule
