// Resolved Dart AST edits for scaffold registration. This program never writes
// project files: all proposed content is returned to the IDE's change planner.
import 'dart:convert';
import 'dart:io';

import 'package:analyzer/dart/analysis/analysis_context_collection.dart';
import 'package:analyzer/dart/analysis/results.dart';
import 'package:analyzer/dart/ast/ast.dart';
import 'package:analyzer/dart/ast/visitor.dart';
import 'package:analyzer/dart/element/type.dart';
import 'package:analyzer/dart/element/element.dart';
import 'package:analyzer/file_system/overlay_file_system.dart';
import 'package:analyzer/file_system/physical_file_system.dart';
import 'package:path/path.dart' as p;

Future<void> main() async {
  try {
    final input =
        jsonDecode(await stdin.transform(utf8.decoder).join())
            as Map<String, dynamic>;
    stdout.write(jsonEncode(await registration(input)));
  } catch (error) {
    stdout.write(jsonEncode({'error': error.toString()}));
    exitCode = 1;
  }
}

class _Nodes extends RecursiveAstVisitor<void> {
  final constructors = <InstanceCreationExpression>[];
  final variables = <VariableDeclaration>[];
  final identifiers = <String>{};
  @override
  void visitInstanceCreationExpression(InstanceCreationExpression node) {
    constructors.add(node);
    super.visitInstanceCreationExpression(node);
  }

  @override
  void visitVariableDeclaration(VariableDeclaration node) {
    variables.add(node);
    identifiers.add(node.name.lexeme);
    super.visitVariableDeclaration(node);
  }

  @override
  void visitSimpleIdentifier(SimpleIdentifier node) {
    identifiers.add(node.name);
    super.visitSimpleIdentifier(node);
  }
}

class _Edit {
  final int offset, length;
  final String text;
  _Edit(this.offset, this.length, this.text);
}

class _File {
  final String path, before;
  final CompilationUnit unit;
  final nodes = _Nodes();
  final edits = <_Edit>[];
  final imports = <String, String>{};
  final Set<String> errors;
  _File(ResolvedUnitResult result)
    : path = result.path,
      before = result.content,
      unit = result.unit,
      errors = _errors(result) {
    unit.accept(nodes);
  }

  String imported(String uri, String hint) {
    if (unit.directives.any((d) => d is PartOfDirective)) {
      throw StateError(
        'Cannot insert imports into part file $path. Select its library instead.',
      );
    }
    for (final directive in unit.directives.whereType<ImportDirective>()) {
      if (directive.uri.stringValue == uri &&
          directive.prefix != null &&
          directive.combinators.isEmpty &&
          directive.configurations.isEmpty &&
          directive.deferredKeyword == null) {
        return directive.prefix!.name;
      }
    }
    if (imports.containsKey(uri)) return imports[uri]!;
    var prefix = hint;
    var index = 2;
    while (nodes.identifiers.contains(prefix) ||
        imports.values.contains(prefix)) {
      prefix = '$hint${index++}';
    }
    imports[uri] = prefix;
    return prefix;
  }

  void append(ListLiteral list, List<String> entries) {
    if (entries.isEmpty) return;
    _collection(
      list.elements,
      list.rightBracket.offset,
      list.constKeyword,
      entries,
    );
  }

  void appendMap(SetOrMapLiteral map, List<String> entries) {
    if (!map.isMap) throw StateError('Expected a routes map in $path');
    if (entries.isEmpty) return;
    _collection(
      map.elements,
      map.rightBracket.offset,
      map.constKeyword,
      entries,
    );
  }

  void _collection(
    List<CollectionElement> elements,
    int end,
    dynamic constKeyword,
    List<String> entries,
  ) {
    if (constKeyword != null) {
      edits.add(
        _Edit(constKeyword.offset as int, constKeyword.length as int, ''),
      );
    }
    if (elements.isNotEmpty && elements.last.endToken.next?.lexeme != ',') {
      edits.add(_Edit(elements.last.end, 0, ','));
    }
    edits.add(_Edit(end, 0, '\n${entries.map((e) => '$e,').join('\n')}\n'));
  }

  String after() {
    if (imports.isNotEmpty) {
      final directives = unit.directives
          .where((d) => d is ImportDirective || d is LibraryDirective)
          .toList();
      var at = 0;
      if (directives.isNotEmpty) {
        final newline = before.indexOf('\n', directives.last.end);
        at = newline < 0 ? before.length : newline + 1;
      } else if (before.startsWith('#!')) {
        at = before.indexOf('\n') + 1;
      }
      edits.add(
        _Edit(
          at,
          0,
          '${at > 0 && before[at - 1] != '\n' ? '\n' : ''}${imports.entries.map((e) => "import '${e.key}' as ${e.value};").join('\n')}\n',
        ),
      );
    }
    // A missing trailing comma and a new entry can share the same boundary.
    // Combine insertions in logical order before applying descending offsets.
    final combined = <int, _Edit>{};
    for (final edit in edits) {
      final previous = combined[edit.offset];
      if (previous != null) {
        if (previous.length != 0 || edit.length != 0)
          throw StateError('Overlapping analyzer edits in $path');
        combined[edit.offset] = _Edit(
          edit.offset,
          0,
          previous.text + edit.text,
        );
      } else {
        combined[edit.offset] = edit;
      }
    }
    final sorted = combined.values.toList()
      ..sort((a, b) => b.offset.compareTo(a.offset));
    var result = before;
    var previous = before.length + 1;
    for (final edit in sorted) {
      if (edit.offset + edit.length > previous)
        throw StateError('Overlapping analyzer edits in $path');
      result = result.replaceRange(
        edit.offset,
        edit.offset + edit.length,
        edit.text,
      );
      previous = edit.offset;
    }
    return result;
  }
}

Set<String> _errors(ResolvedUnitResult result) => result.diagnostics
    .where((d) => d.diagnosticCode.severity.name == 'ERROR')
    .map((d) => '${d.diagnosticCode.lowerCaseName}: ${d.message}')
    .toSet();

String? _class(InstanceCreationExpression node) =>
    node.constructorName.element?.enclosingElement.name;
String _library(InstanceCreationExpression node) =>
    node.constructorName.element?.library.uri.toString() ?? '';
Expression? _argument(InstanceCreationExpression node, String name) {
  for (final argument
      in node.argumentList.arguments.whereType<NamedExpression>()) {
    if (argument.name.label.name == name) return argument.expression;
  }
  return null;
}

String? _string(Expression expression) =>
    expression.computeConstantValue()?.value?.toStringValue();
void _checkElements(Iterable<CollectionElement> elements, String path) {
  if (elements.any(
    (e) => e is SpreadElement || e is IfElement || e is ForElement,
  )) {
    throw StateError(
      'Dynamic/spread registration collections in $path cannot be updated safely. Select a literal registration collection.',
    );
  }
}

Expression _literal(_File file, Expression expression) {
  if (expression is ParenthesizedExpression)
    return _literal(file, expression.expression);
  if (expression is SimpleIdentifier) {
    final element = expression.element;
    final variableElement = element is PropertyAccessorElement
        ? element.variable
        : element;
    for (final variable in file.nodes.variables) {
      if (variable.declaredFragment?.element == variableElement &&
          variable.initializer != null) {
        if (variable.initializer is SimpleIdentifier)
          throw StateError(
            'Indirect registration collection in ${file.path}. Use a literal or a direct literal variable.',
          );
        return _literal(file, variable.initializer!);
      }
    }
  }
  return expression;
}

class _Target {
  final _File file;
  final InstanceCreationExpression node;
  final String kind;
  _Target(this.file, this.node, this.kind);
}

_Target? _choose(
  List<_Target> targets,
  String? selected,
  String root,
  String label,
) {
  if (selected != null) {
    final parts = selected.split('#');
    final path = p.normalize(p.join(root, parts.first));
    if (!p.isWithin(root, path))
      throw StateError('$label target must be inside the package');
    targets = targets.where((t) {
      if (t.file.path != path) return false;
      if (parts.length == 1) return true;
      AstNode? parent = t.node;
      while (parent != null && parent is! VariableDeclaration) {
        parent = parent.parent;
      }
      return parent is VariableDeclaration && parent.name.lexeme == parts[1];
    }).toList();
    if (targets.isEmpty)
      throw StateError(
        'No supported $label target matches $selected. Check resolved imports and registration settings.',
      );
  }
  if (targets.length > 1) {
    throw StateError(
      'Multiple $label targets: ${targets.map((t) => '${p.relative(t.file.path, from: root)}:${t.node.offset}').join(', ')}. Pin registration.$label in .sparrow/scaffold.yaml (file.dart or file.dart#variable).',
    );
  }
  return targets.isEmpty ? null : targets.single;
}

Future<Map<String, Object?>> registration(Map<String, dynamic> request) async {
  final root = p.normalize(request['root'] as String);
  final name = request['name'] as String;
  final snake = request['snake'] as String;
  final routePath = '/${snake.replaceAll('_', '-')}';
  final screenUri = request['screen_uri'] as String;
  final stateUri = request['state_uri'] as String;
  final repositoryUri = request['repository_uri'] as String;
  final provider = OverlayResourceProvider(PhysicalResourceProvider.INSTANCE);
  final sources = (request['sources'] as List).cast<Map<String, dynamic>>();
  for (final source in [
    ...sources,
    ...(request['overlays'] as List).cast<Map<String, dynamic>>(),
  ]) {
    final path = source['path'] as String;
    if (!p.isWithin(root, path) || !p.isAbsolute(path))
      throw StateError('Source is outside the package');
    provider.setOverlay(
      path,
      content: source['content'] as String,
      modificationStamp: 1,
    );
  }
  var collection = AnalysisContextCollection(
    includedPaths: [root],
    resourceProvider: provider,
  );
  try {
    final files = <_File>[];
    final routers = <_Target>[];
    final providers = <_Target>[];
    final apps = <_Target>[];
    for (final source in sources) {
      final path = source['path'] as String;
      final result = await collection
          .contextFor(path)
          .currentSession
          .getResolvedUnit(path);
      if (result is! ResolvedUnitResult)
        throw StateError('Could not resolve $path. Run Pub Get first.');
      final file = _File(result);
      files.add(file);
      for (final directive
          in file.unit.directives.whereType<ImportDirective>()) {
        final uri = directive.uri.stringValue ?? '';
        if ([
              'package:flutter/',
              'package:go_router/',
              'package:provider/',
            ].any(uri.startsWith) &&
            (directive.libraryImport?.importedLibrary == null || !provider.getFile(directive.libraryImport!.importedLibrary!.firstFragment.source.fullName).exists)) {
          throw StateError(
            'Cannot resolve $uri in $path. Run Pub Get before automatic registration.',
          );
        }
      }
      for (final node in file.nodes.constructors) {
        final library = _library(node), type = _class(node);
        if (type == 'GoRouter' && library.startsWith('package:go_router/'))
          routers.add(_Target(file, node, 'go_router'));
        if (type == 'MaterialApp' &&
            library.startsWith('package:flutter/') &&
            node.constructorName.name == null)
          routers.add(_Target(file, node, 'material'));
        if (type == 'MaterialApp' && library.startsWith('package:flutter/'))
          apps.add(_Target(file, node, 'wrap'));
        if (type == 'MultiProvider' && library.startsWith('package:provider/'))
          providers.add(_Target(file, node, 'provider'));
      }
    }
    final settings = (request['settings'] as Map?) ?? const {};
    final router = _choose(
      routers,
      settings['router'] as String?,
      root,
      'router',
    );
    final multi = _choose(
      providers.isEmpty && request['provider_available'] == true
          ? apps
          : providers,
      settings['providers'] as String?,
      root,
      'providers',
    );
    final notes = <String>[];
    for (final target in [if(router!=null)router,if(multi!=null)multi]) {
      if(target.file.errors.isNotEmpty) throw StateError('Fix analyzer errors in registration target ${target.file.path} before scaffolding: ${target.file.errors.join('; ')}');
    }
    if (multi != null) {
      final f = multi.file;
      final expression = _argument(multi.node, 'providers');
      final list = expression == null ? null : _literal(f, expression);
      if (multi.kind != 'wrap' && list is! ListLiteral)
        throw StateError(
          'MultiProvider.providers must be a list literal or a direct same-file variable in ${f.path}',
        );
      if (list is ListLiteral) _checkElements(list.elements, f.path);
      final found = <String>{};
      for (final element
          in list is ListLiteral
              ? list.elements
              : const <CollectionElement>[]) {
        if (element is! InstanceCreationExpression) continue;
        final type = element.staticType;
        if (type is InterfaceType &&
            type.element.library.uri.toString().startsWith(
              'package:provider/',
            ) &&
            type.typeArguments.isNotEmpty) {
          final argument = type.typeArguments.first;
          if (argument is InterfaceType) {
            if (argument.element.name == '${name}Repository' &&
                argument.element.library.uri.toString() == repositoryUri)
              found.add('repository');
            if (argument.element.name == '${name}ViewModel' &&
                argument.element.library.uri.toString() == stateUri)
              found.add('state');
          }
        }
      }
      final entries = <String>[];
      if (found.contains('state') && !found.contains('repository'))
        throw StateError(
          'The view model is already registered without its repository in ${f.path}. Register the repository before the existing view model.',
        );
      if (!found.contains('repository')) {
        final pkg = f.imported(
          'package:provider/provider.dart',
          'sparrowProvider',
        );
        final repo = f.imported(repositoryUri, 'sparrow${name}Repository');
        entries.add(
          '$pkg.Provider<$repo.${name}Repository>(create: (_) => $repo.InMemory${name}Repository())',
        );
      }
      if (!found.contains('state')) {
        final pkg = f.imported(
          'package:provider/provider.dart',
          'sparrowProvider',
        );
        final repo = f.imported(repositoryUri, 'sparrow${name}Repository');
        final state = f.imported(stateUri, 'sparrow${name}State');
        entries.add(
          '$pkg.ChangeNotifierProvider<$state.${name}ViewModel>(create: (context) => $state.${name}ViewModel($pkg.Provider.of<$repo.${name}Repository>(context, listen: false)))',
        );
      }
      if (list is ListLiteral) {
        f.append(list, entries);
      } else {
        final pkg = f.imported(
          'package:provider/provider.dart',
          'sparrowProvider',
        );
        f.edits.add(
          _Edit(
            multi.node.offset,
            0,
            '$pkg.MultiProvider(providers: [\n${entries.map((e) => '$e,').join('\n')}\n], child: ',
          ),
        );
        f.edits.add(_Edit(multi.node.end, 0, ')'));
      }
      notes.add(
        'Provider: repository and view model registered in ${p.relative(f.path, from: root)}${multi.kind == 'wrap' ? ' (created MultiProvider around MaterialApp)' : ' (existing registrations retained)'}.',
      );
    } else {
      notes.add(
        'No resolved MultiProvider found; screen retains its constructor-based dependency injection.',
      );
    }
    if (router != null) {
      final f = router.file;
      final expression = _argument(router.node, 'routes');
      final routes = expression == null ? null : _literal(f, expression);
      var exists = false;
      if (router.kind == 'go_router') {
        if (routes is! ListLiteral)
          throw StateError(
            'GoRouter.routes must be a list literal or a direct same-file variable in ${f.path}. Generated/indirect routers need an explicit supported target.',
          );
        _checkElements(routes.elements, f.path);
        final routeNodes = _Nodes();
        routes.accept(routeNodes);
        for (final element in routeNodes.constructors) {
          if (_class(element) == 'GoRoute' &&
              _library(element).startsWith('package:go_router/')) {
            final path = _argument(element, 'path');
            if (path != null && _string(path) == null)
              throw StateError(
                'Cannot evaluate a route path in ${f.path}. Use constant route paths.',
              );
            if (path != null && _string(path) == routePath) {
              final nested = _Nodes();
              element.accept(nested);
              exists = nested.constructors.any(
                (n) => _class(n) == '${name}Screen' && _library(n) == screenUri,
              );
              if (!exists)
                throw StateError(
                  'Route $routePath already exists with a different destination in ${f.path}',
                );
            }
          }
        }
      } else if (routes != null) {
        if (routes is! SetOrMapLiteral || !routes.isMap)
          throw StateError(
            'MaterialApp.routes must be a map literal or a direct same-file variable in ${f.path}',
          );
        _checkElements(routes.elements, f.path);
        for (final entry in routes.elements.whereType<MapLiteralEntry>()) {
          final key = entry.key;
          if (_string(key) == null)
            throw StateError(
              'Cannot evaluate a route key in ${f.path}. Use constant route paths.',
            );
          if (_string(key) == routePath) {
            final nested = _Nodes();
            entry.value.accept(nested);
            exists = nested.constructors.any(
              (n) => _class(n) == '${name}Screen' && _library(n) == screenUri,
            );
            if (!exists)
              throw StateError(
                'Route $routePath already exists with a different destination in ${f.path}',
              );
          }
        }
      }
      if (!exists) {
        final screen = f.imported(screenUri, 'sparrow${name}Screen');
        var widget = '$screen.${name}Screen()';
        if (multi != null) {
          final pkg = f.imported(
            'package:provider/provider.dart',
            'sparrowProvider',
          );
          final state = f.imported(stateUri, 'sparrow${name}State');
          widget =
              '$screen.${name}Screen(viewModel: $pkg.Provider.of<$state.${name}ViewModel>(context, listen: false))';
        }
        if (router.kind == 'go_router') {
          final pkg = f.imported(
            'package:go_router/go_router.dart',
            'sparrowRouter',
          );
          f.append(routes as ListLiteral, [
            "$pkg.GoRoute(path: '$routePath', builder: (context, state) => $widget)",
          ]);
        } else {
          final entry = "'$routePath': (context) => $widget";
          if (routes is SetOrMapLiteral) {
            f.appendMap(routes, [entry]);
          } else {
            final args = router.node.argumentList;
            if (args.arguments.isNotEmpty &&
                args.arguments.last.endToken.next?.lexeme != ',')
              f.edits.add(_Edit(args.arguments.last.end, 0, ','));
            f.edits.add(
              _Edit(args.rightParenthesis.offset, 0, '\nroutes: {$entry},\n'),
            );
          }
        }
      }
      notes.add(
        'Router: $routePath → ${name}Screen in ${p.relative(f.path, from: root)}.',
      );
    } else {
      notes.add(
        'No resolved MaterialApp/GoRouter registration target found. The generated route factory remains available.',
      );
    }
    final changes = <Map<String, Object?>>[];
    for (final file in files.where(
      (f) => f.edits.isNotEmpty || f.imports.isNotEmpty,
    )) {
      final after = file.after();
      provider.setOverlay(file.path, content: after, modificationStamp: 2);
      changes.add({'path': file.path, 'before': file.before, 'after': after});
    }
    // Resolve the proposed state entirely in memory. A newly introduced error
    // prevents every edit from reaching the filesystem transaction.
    await collection.dispose();
    collection = AnalysisContextCollection(
      includedPaths: [root],
      resourceProvider: provider,
    );
    for (final change in changes) {
      final path = change['path'] as String;
      final result = await collection
          .contextFor(path)
          .currentSession
          .getResolvedUnit(path);
      if (result is! ResolvedUnitResult)
        throw StateError('Could not validate proposed edits for $path');
      final previous = files.firstWhere((f) => f.path == path).errors;
      final errors = _errors(result).difference(previous);
      if (errors.isNotEmpty)
        throw StateError(
          'Registration would introduce analyzer errors in $path: ${errors.join('; ')}',
        );
    }
    return {'changes': changes, 'notes': notes};
  } finally {
    await collection.dispose();
  }
}
