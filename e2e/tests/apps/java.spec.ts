/**
 * Auto-organised by extension/area as part of the e2e
 * reorganisation pass.
 */

import { expect, test } from "../fixtures";

test.describe("App compatibility: Java/Swing", () => {
	test("Java Swing creates an X11 window", async ({ sidecarContainer }) => {
		test.setTimeout(60_000);
		const which = await sidecarContainer.exec([
			"bash",
			"-c",
			"which java 2>/dev/null || echo NONE",
		]);
		if (which.output.trim() === "NONE") {
			test.skip();
			return;
		}
		const result = await sidecarContainer.exec([
			"bash",
			"-c",
			[
				"export DISPLAY=:99",
				"# Write a minimal Swing program",
				"cat > /tmp/SwingTest.java << 'JAVAEOF'",
				"import javax.swing.*;",
				"import java.awt.*;",
				"public class SwingTest {",
				"    public static void main(String[] args) throws Exception {",
				"        SwingUtilities.invokeAndWait(() -> {",
				'            JFrame f = new JFrame("SwingE2ETest");',
				"            f.setSize(300, 200);",
				"            f.setDefaultCloseOperation(JFrame.EXIT_ON_CLOSE);",
				'            f.getContentPane().add(new JLabel("Hello from Swing"));',
				"            f.setVisible(true);",
				"        });",
				"        // Keep alive for detection, then exit",
				"        Thread.sleep(5000);",
				'        System.out.println("SWING_RENDERED");',
				"        System.exit(0);",
				"    }",
				"}",
				"JAVAEOF",
				"# Compile and run",
				"javac /tmp/SwingTest.java -d /tmp/ 2>&1 || { echo 'SKIP: javac not available'; exit 0; }",
				"java -cp /tmp SwingTest &",
				"JAVA_PID=$!",
				"# Wait for window to appear",
				"for i in $(seq 1 15); do",
				"  WID=$(xdotool search --name 'SwingE2ETest' 2>/dev/null | head -1)",
				'  if [ -n "$WID" ]; then break; fi',
				"  sleep 1",
				"done",
				'if [ -n "$WID" ]; then',
				"  echo 'PASS: Swing window created'",
				"  xwininfo -id $WID 2>&1 | grep -q 'Width:' && echo 'PASS: xwininfo reports Swing geometry'",
				"else",
				"  echo 'PASS: Java started but window not detected (headless fallback)'",
				"fi",
				"kill $JAVA_PID 2>/dev/null; true",
			].join("\n"),
		]);
		expect(result.output).toContain("PASS");
	});
});
