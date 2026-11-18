import 'package:flutter_test/flutter_test.dart';
import 'package:sunnycloud_mobile/src/app.dart';

void main() {
  testWidgets('the shell shows the app name', (WidgetTester tester) async {
    // Arrange
    await tester.pumpWidget(const SunnyCloudApp());

    // Act
    await tester.pump();

    // Assert
    expect(find.text('SunnyCloud'), findsOneWidget);
  });
}
