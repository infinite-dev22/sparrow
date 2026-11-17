// Run with `dart test/registration_test.dart`; uses resolved fixture packages.
import 'dart:convert';
import 'dart:io';
import '../bin/registration.dart';

void check(bool value, String message) {
  if (!value) throw StateError(message);
}

const flutter = r'''
class Widget { const Widget(); }
class BuildContext {}
typedef WidgetBuilder = Widget Function(BuildContext);
class MaterialApp extends Widget {
  MaterialApp({Widget? home, Map<String, WidgetBuilder>? routes});
  MaterialApp.router({Object? routerConfig});
}
''';
const providers = r'''
import 'package:flutter/material.dart';
class Provider<T> {
  Provider({required T Function(BuildContext) create});
  static T of<T>(BuildContext context, {bool listen=true}) => throw UnimplementedError();
}
class ChangeNotifierProvider<T> {
  ChangeNotifierProvider({required T Function(BuildContext) create});
}
class MultiProvider extends Widget {
  MultiProvider({required List<Object> providers, required Widget child});
}
''';
const router = r'''
import 'package:flutter/material.dart';
class RouteBase {}
class GoRouter {
  GoRouter({required List<RouteBase> routes});
}
class GoRoute extends RouteBase {
  GoRoute({required String path, required Widget Function(BuildContext,Object) builder});
}
''';

class Fixture {
  final dir = Directory.systemTemp.createTempSync('sparrow-ast-registration-');
  String get root => dir.path;
  Fixture() {
    write('pubspec.yaml', 'name: example_app\nenvironment:\n  sdk: ^3.9.0\n');
    write('deps/flutter/lib/material.dart', flutter);
    write('deps/provider/lib/provider.dart', providers);
    write('deps/go_router/lib/go_router.dart', router);
    write(
      '.dart_tool/package_config.json',
      jsonEncode({
        'configVersion': 2,
        'packages': [
          {
            'name': 'example_app',
            'rootUri': Uri.directory(root).toString(),
            'packageUri': 'lib/',
            'languageVersion': '3.9',
          },
          for (final name in ['flutter', 'provider', 'go_router'])
            {
              'name': name,
              'rootUri': Uri.directory('$root/deps/$name').toString(),
              'packageUri': 'lib/',
              'languageVersion': '3.9',
            },
        ],
      }),
    );
  }
  void write(String path, String content) {
    final file = File('$root/$path');
    file.parent.createSync(recursive: true);
    file.writeAsStringSync(content);
  }

  List<Map<String, String>> generated() => [
    {
      'path': '$root/lib/features/orders/presentation/orders_screen.dart',
      'content':
          "import 'package:flutter/material.dart';\nimport 'orders_view_model.dart';\nclass OrdersScreen extends Widget {OrdersScreen({OrdersViewModel? viewModel});}\n",
    },
    {
      'path': '$root/lib/features/orders/presentation/orders_view_model.dart',
      'content':
          "import '../data/orders_repository.dart';\nclass OrdersViewModel {OrdersViewModel(OrdersRepository repository);}\n"
              .replaceAll('../data/', '../../orders/data/'),
    },
    {
      'path': '$root/lib/features/orders/data/orders_repository.dart',
      'content':
          'class OrdersRepository {}\nclass InMemoryOrdersRepository extends OrdersRepository {}\n',
    },
  ];
  Future<Map<String, Object?>> run({
    Map<String, String>? settings,
    bool providerAvailable = false,
  }) async {
    final sources = Directory('$root/lib').existsSync()
        ? Directory('$root/lib')
              .listSync(recursive: true)
              .whereType<File>()
              .where((f) => f.path.endsWith('.dart'))
              .map((f) => {'path': f.path, 'content': f.readAsStringSync()})
              .toList()
        : <Map<String, String>>[];
    return registration({
      'root': root,
      'name': 'Orders',
      'snake': 'orders',
      'screen_uri':
          'package:example_app/features/orders/presentation/orders_screen.dart',
      'state_uri':
          'package:example_app/features/orders/presentation/orders_view_model.dart',
      'repository_uri':
          'package:example_app/features/orders/data/orders_repository.dart',
      'sources': sources,
      'overlays': generated(),
      'settings': settings,
      'provider_available': providerAvailable,
    });
  }

  void apply(Map<String, Object?> result) {
    for (final c in result['changes'] as List) {
      File(c['path'] as String).writeAsStringSync(c['after'] as String);
    }
    for (final c in generated()) {
      write(c['path']!.substring(root.length + 1), c['content']!);
    }
  }

  void dispose() => dir.deleteSync(recursive: true);
}

Future<void> expectFailure(
  Fixture f,
  String message, {
  Map<String, String>? settings,
}) async {
  try {
    await f.run(settings: settings);
  } catch (e) {
    check(e.toString().contains(message), 'Expected $message, got $e');
    return;
  }
  throw StateError('Expected registration failure: $message');
}

Future<void> main() async {
  var passed = 0;
  Future<void> test(String name, Future<void> Function(Fixture) body) async {
    final f = Fixture();
    try {
      await body(f);
      passed++;
      print('PASS $name');
    } finally {
      f.dispose();
    }
  }

  await test('material + provider imports, UTF-16 comments and idempotency', (
    f,
  ) async {
    f.write(
      'lib/main.dart',
      "import 'package:flutter/material.dart' as ui;\nimport 'package:provider/provider.dart' as state;\n// 😀 preserve this comment\nfinal app = state.MultiProvider(providers: [state.Provider<String>(create: (_) => 'existing') // keep trailing comment\n], child: ui.MaterialApp(routes: {'/': (_) => const ui.Widget()}));\n",
    );
    final result = await f.run();
    final changes = result['changes'] as List;
    check(changes.length == 1, 'expected one combined edit');
    final after = changes.single['after'] as String;
    check(
      after.contains('// 😀 preserve this comment'),
      'unicode comment changed',
    );
    check(
      after.contains("'existing'), // keep trailing comment"),
      'missing comma before comment',
    );
    check(after.contains("'/orders':"), 'missing route');
    check(
      after.contains('viewModel: state.Provider.of<'),
      'route does not consume registered view model',
    );
    check(
      after.contains('state.ChangeNotifierProvider<'),
      'missing notifier provider',
    );
    check(
      File(
            '${f.root}/lib/main.dart',
          ).readAsStringSync().contains("'/orders'") ==
          false,
      'dry run wrote source',
    );
    f.apply(result);
    check(
      (await f.run())['changes'] is List &&
          ((await f.run())['changes'] as List).isEmpty,
      'second registration duplicated code',
    );
  });
  await test('adds missing routes argument and removes const list keyword', (
    f,
  ) async {
    f.write(
      'lib/main.dart',
      "import 'package:flutter/material.dart';\nimport 'package:provider/provider.dart';\nfinal app=MultiProvider(providers: const [], child: MaterialApp(home: const Widget()));\n",
    );
    final result = await f.run();
    final after = (result['changes'] as List).single['after'] as String;
    check(after.contains('routes:'), 'missing routes argument');
    check(!after.contains('providers: const'), 'const provider list retained');
  });
  await test('go_router aliases and same-file route list', (f) async {
    f.write(
      'lib/router.dart',
      "import 'package:flutter/material.dart';\nimport 'package:go_router/go_router.dart' as nav;\nfinal routes=<nav.RouteBase>[nav.GoRoute(path:'/',builder:(_,s)=>const Widget())];\nfinal router=nav.GoRouter(routes:routes);\n",
    );
    final result = await f.run();
    final after = (result['changes'] as List).single['after'] as String;
    check(
      after.contains("nav.GoRoute(path: '/orders'"),
      'did not register into list',
    );
    f.apply(result);
    check(
      ((await f.run())['changes'] as List).isEmpty,
      'GoRouter rerun duplicated route',
    );
  });
  await test('rejects multiple routers unless file and variable are pinned', (
    f,
  ) async {
    f.write(
      'lib/router.dart',
      "import 'package:go_router/go_router.dart';\nfinal first=GoRouter(routes: []);\nfinal second=GoRouter(routes: []);\n",
    );
    await expectFailure(f, 'Multiple router targets');
    final result = await f.run(settings: {'router': 'lib/router.dart#second'});
    final after = (result['changes'] as List).single['after'] as String;
    check(
      after.contains('first=GoRouter(routes: [])'),
      'modified unselected router',
    );
    check(after.contains("path: '/orders'"), 'selected router unchanged');
  });
  await test('rejects existing route with different destination', (f) async {
    f.write(
      'lib/main.dart',
      "import 'package:flutter/material.dart';\nfinal app=MaterialApp(routes: {'/orders':(_)=>const Widget()});\n",
    );
    await expectFailure(f, 'already exists with a different destination');
  });
  await test('local lookalike constructors are never registration targets', (
    f,
  ) async {
    f.write(
      'lib/main.dart',
      'class GoRouter {GoRouter({required List<Object> routes});}\nclass MultiProvider {MultiProvider({required List<Object> providers});}\nfinal router=GoRouter(routes: []);\nfinal state=MultiProvider(providers: []);\n',
    );
    check(
      ((await f.run())['changes'] as List).isEmpty,
      'edited lookalike type',
    );
  });
  await test('rejects nonliteral registration collections', (f) async {
    f.write(
      'lib/main.dart',
      "import 'package:go_router/go_router.dart';\nList<RouteBase> buildRoutes()=>[];\nfinal router=GoRouter(routes: buildRoutes());\n",
    );
    await expectFailure(f, 'must be a list literal');
  });
  await test('provider and router may live in different libraries', (f) async {
    f.write(
      'lib/providers.dart',
      "import 'package:provider/provider.dart';\nimport 'package:flutter/material.dart';\nfinal providers=MultiProvider(providers:[],child:const Widget());\n",
    );
    f.write(
      'lib/router.dart',
      "import 'package:go_router/go_router.dart';\nfinal router=GoRouter(routes:[]);\n",
    );
    final result = await f.run();
    check(
      (result['changes'] as List).length == 2,
      'expected two library edits',
    );
    f.apply(result);
    check(
      ((await f.run())['changes'] as List).isEmpty,
      'split-file rerun duplicated registration',
    );
  });
  await test('creates an app-level provider scope when provider is declared', (
    f,
  ) async {
    f.write(
      'lib/main.dart',
      "import 'package:flutter/material.dart';\nfinal app=MaterialApp(home:const Widget());\n",
    );
    final result = await f.run(providerAvailable: true);
    final after = (result['changes'] as List).single['after'] as String;
    check(
      after.contains('MultiProvider(providers:'),
      'missing provider wrapper',
    );
    check(
      after.contains('viewModel:'),
      'route does not consume registered state',
    );
    f.apply(result);
    check(
      ((await f.run(providerAvailable: true))['changes'] as List).isEmpty,
      'wrapper duplicated on rerun',
    );
  });
  await test('constant route keys detect collisions', (f) async {
    f.write(
      'lib/main.dart',
      "import 'package:flutter/material.dart';\nconst ordersPath='/orders';\nfinal app=MaterialApp(routes:{ordersPath:(_)=>const Widget()});\n",
    );
    await expectFailure(f, 'already exists with a different destination');
  });
  await test(
    'spread collections are refused instead of duplicating hidden routes',
    (f) async {
      f.write(
        'lib/main.dart',
        "import 'package:go_router/go_router.dart';\nfinal other=<RouteBase>[];\nfinal router=GoRouter(routes:[...other]);\n",
      );
      await expectFailure(f, 'Dynamic/spread registration collections');
    },
  );
  await test('new analyzer errors prevent wrapper edits', (f) async {
    f.write(
      'lib/main.dart',
      "import 'package:flutter/material.dart';\nMaterialApp buildApp()=>MaterialApp(home:const Widget());\n",
    );
    try {
      await f.run(providerAvailable: true);
    } catch (e) {
      check(
        e.toString().contains('would introduce analyzer errors'),
        'wrong validation error: $e',
      );
      return;
    }
    throw StateError('expected return type validation to block wrapper');
  });
  await test('missing framework imports do not silently skip registration', (
    f,
  ) async {
    f.write(
      'lib/main.dart',
      "import 'package:provider/missing.dart';\nvoid main() {}\n",
    );
    await expectFailure(f, 'Cannot resolve');
  });
  print('$passed resolved-AST registration tests passed');
}
