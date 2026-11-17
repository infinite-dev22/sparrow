//! The first complete pack uses only Flutter APIs and flutter_test.
use super::engine::{Dependency, Pack, PackFile};
pub fn pack() -> Pack {
    let files = [
        (
            "{{model_dir}}/{{snake}}_item.dart",
            r#"class {{name}}Item {
  const {{name}}Item({required this.id, required this.title});
  final String id;
  final String title;
}
"#,
        ),
        (
            "{{repository_dir}}/{{snake}}_repository.dart",
            r#"import 'package:{{package}}/{{model_import}}/{{snake}}_item.dart';

abstract interface class {{name}}Repository {
  Future<List<{{name}}Item>> load();
}

class InMemory{{name}}Repository implements {{name}}Repository {
  @override
  Future<List<{{name}}Item>> load() async => const [];
}
"#,
        ),
        (
            "{{state_dir}}/{{snake}}_view_model.dart",
            r#"import 'package:flutter/foundation.dart';
import 'package:{{package}}/{{model_import}}/{{snake}}_item.dart';
import 'package:{{package}}/{{repository_import}}/{{snake}}_repository.dart';

class {{name}}ViewModel extends ChangeNotifier {
  {{name}}ViewModel(this._repository);
  final {{name}}Repository _repository;
  List<{{name}}Item> _items = const [];
  bool _loading = false;
  bool _disposed = false;
  String? _error;

  List<{{name}}Item> get items => _items;
  bool get loading => _loading;
  String? get error => _error;

  Future<void> load() async {
    if (_loading || _disposed) return;
    _loading = true;
    _error = null;
    notifyListeners();
    try {
      final items = await _repository.load();
      if (!_disposed) _items = List.unmodifiable(items);
    } catch (error) {
      if (!_disposed) _error = error.toString();
    } finally {
      if (!_disposed) {
        _loading = false;
        notifyListeners();
      }
    }
  }

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
  }
}
"#,
        ),
        (
            "{{screen_dir}}/{{snake}}_screen.dart",
            r#"import 'package:flutter/material.dart';
import 'package:{{package}}/{{repository_import}}/{{snake}}_repository.dart';
import '{{snake}}_view_model.dart';

class {{name}}Screen extends StatefulWidget {
  const {{name}}Screen({super.key, this.repository});
  final {{name}}Repository? repository;
  @override
  State<{{name}}Screen> createState() => _{{name}}ScreenState();
}

class _{{name}}ScreenState extends State<{{name}}Screen> {
  late final {{name}}ViewModel _model;
  @override
  void initState() {
    super.initState();
    _model = {{name}}ViewModel(widget.repository ?? InMemory{{name}}Repository());
    _model.load();
  }

  @override
  void dispose() {
    _model.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(title: const Text('{{name}}')),
    body: ListenableBuilder(
      listenable: _model,
      builder: (context, _) {
        if (_model.loading) {
          return const Center(child: CircularProgressIndicator());
        }
        if (_model.error != null) {
          return Center(child: Column(mainAxisSize: MainAxisSize.min, children: [
            Text(_model.error!),
            FilledButton(onPressed: _model.load, child: const Text('Retry')),
          ]));
        }
        if (_model.items.isEmpty) {
          return const Center(child: Text('No items yet'));
        }
        return RefreshIndicator(
          onRefresh: _model.load,
          child: ListView.builder(
            physics: const AlwaysScrollableScrollPhysics(),
            itemCount: _model.items.length,
            itemBuilder: (context, index) => ListTile(
              key: ValueKey(_model.items[index].id),
              title: Text(_model.items[index].title),
            ),
          ),
        );
      },
    ),
  );
}
"#,
        ),
        (
            "{{screen_dir}}/{{snake}}_route.dart",
            r#"import 'package:flutter/material.dart';
import 'package:{{package}}/{{repository_import}}/{{snake}}_repository.dart';
import '{{snake}}_screen.dart';

Route<void> create{{name}}Route({{{name}}Repository? repository}) =>
    MaterialPageRoute<void>(builder: (_) => {{name}}Screen(repository: repository));
"#,
        ),
        (
            "test/features/{{snake}}/{{snake}}_view_model_test.dart",
            r#"import 'dart:async';
import 'package:flutter_test/flutter_test.dart';
import 'package:{{package}}/{{model_import}}/{{snake}}_item.dart';
import 'package:{{package}}/{{repository_import}}/{{snake}}_repository.dart';
import 'package:{{package}}/{{screen_import}}/{{snake}}_view_model.dart';

class _Repository implements {{name}}Repository {
  final response = Completer<List<{{name}}Item>>();
  @override
  Future<List<{{name}}Item>> load() => response.future;
}

void main() {
  test('loads data and notifies loading transitions', () async {
    final repository = _Repository();
    final model = {{name}}ViewModel(repository);
    addTearDown(model.dispose);
    final loading = <bool>[];
    model.addListener(() => loading.add(model.loading));
    final result = model.load();
    expect(model.loading, isTrue);
    repository.response.complete(const [{{name}}Item(id: '1', title: 'First')]);
    await result;
    expect(model.items.single.title, 'First');
    expect(model.error, isNull);
    expect(loading, [true, false]);
  });

  test('exposes repository failures', () async {
    final repository = _Repository();
    final model = {{name}}ViewModel(repository);
    addTearDown(model.dispose);
    final result = model.load();
    repository.response.completeError(StateError('offline'));
    await result;
    expect(model.error, contains('offline'));
    expect(model.loading, isFalse);
  });

  test('does not notify after disposal', () async {
    final repository = _Repository();
    final model = {{name}}ViewModel(repository);
    final result = model.load();
    model.dispose();
    repository.response.complete(const []);
    await result;
  });
}
"#,
        ),
        (
            "test/features/{{snake}}/{{snake}}_screen_test.dart",
            r#"import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:{{package}}/{{screen_import}}/{{snake}}_screen.dart';

void main() {
  testWidgets('shows the empty state', (tester) async {
    await tester.pumpWidget(const MaterialApp(home: {{name}}Screen()));
    await tester.pumpAndSettle();
    expect(find.text('No items yet'), findsOneWidget);
    expect(find.text('{{name}}'), findsOneWidget);
  });
}
"#,
        ),
    ];
    Pack {
        schema: 1, id: "sparrow.feature.mvvm".into(), version: "1.0.0".into(),
        title: "Feature · MVVM / ChangeNotifier".into(),
        description: "Screen + view model + repository + model + route factory + unit/widget tests. Includes loading, error, empty and data states. Uses ChangeNotifier explicitly; existing state packages are reported, not silently migrated. Connect the generated route factory to your router after applying.".into(),
        files: files.into_iter().map(|(path,content)|PackFile{path:path.into(),content:content.into()}).collect(),
        dependencies: vec![Dependency{package:"flutter_test".into(),version:None,sdk:Some("flutter".into()),dev:true}],
    }
}
