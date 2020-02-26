import 'package:flutter/material.dart';
import 'package:validators/validators.dart';

void main() => runApp(MyApp());

class MyApp extends StatelessWidget {
  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Vertex',
      theme: ThemeData(
        primarySwatch: Colors.teal,
      ),
      home: LoginPage(title: 'Log in to Vertex'),
    );
  }
}

/// Subclass of widget that holds all state for app
class LoginPage extends StatefulWidget {
  LoginPage({Key key, this.title}) : super(key: key);

  final String title;

  @override
  LoginPageState createState() => LoginPageState();
}

class LoginPageState extends State<LoginPage> {
  /// Called on setState
  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Text(widget.title),
      ),
      body: Center(
        child: LoginForm()
      ),
    );
  }
}

class LoginForm extends StatefulWidget {
  @override
  LoginFormState createState() => LoginFormState();
}

class LoginFormState extends State<LoginForm> {
  final _formKey = GlobalKey<FormState>();

  @override
  Widget build(BuildContext context) {
    return Form(
        key: this._formKey,
        child: ListView(
          children: <Widget>[
            LoginFormEntry(
              name: "Instance",
              top: 20,
              validator: (value) {
                if (!isURL(value)) {
                  return "Instance must be a valid URL";
                }
                return null;
              },
            ),
            LoginFormEntry(
              name: "Username",
              validator: (value) {
                if (value.isEmpty) {
                  return "Username cannot be empty";
                }
                return null;
              },
            ),
            LoginFormEntry(
              name: "Password",
              password: true,
              validator: (value) {
                  if (value.isEmpty) {
                    return "Password cannot be empty";
                  }
                  return null;
              },
            ),
            Padding(
              padding: const EdgeInsets.all(8.0),
              child: RaisedButton(
                onPressed: () {
                  if (this._formKey.currentState.validate()) {
                    Scaffold
                        .of(context)
                        .showSnackBar(SnackBar(content: Text("Sending to server")));
                  }
                },
                child: Text("Register")
              ),
            ),
          ]
        )
    );
  }
}

typedef EntryValidator = String Function(String value);

class LoginFormEntry extends StatelessWidget {
  final String name;
  final EntryValidator validator;
  final double top;
  final bool password;

  const LoginFormEntry({ this.name, this.validator, this.top = 0, this.password = false });

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: EdgeInsets.fromLTRB(20, this.top, 20, 20),
      child: TextFormField(
        decoration: InputDecoration(
          labelText: this.name,
          hintText: this.name,
        ),
        obscureText: this.password,
        autocorrect: !this.password,
        validator: this.validator,
      ),
    );
  }
}
